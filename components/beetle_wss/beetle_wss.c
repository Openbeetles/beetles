#include "beetle_wss.h"

#include <ctype.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#include "esp_crt_bundle.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_tls.h"
#include "lwip/sockets.h"

#define BEETLE_WSS_LOG_TAG "beetle_wss"
#define BEETLE_WSS_GUID "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
#define BEETLE_WSS_DEFAULT_PORT 443
#define BEETLE_WSS_DEFAULT_CONNECT_TIMEOUT_MS 10000U
#define BEETLE_WSS_DEFAULT_IO_TIMEOUT_MS 30000U
#define BEETLE_WSS_DEFAULT_MAX_MESSAGE_BYTES (256U * 1024U)
#define BEETLE_WSS_HANDSHAKE_MAX_HEADER_BYTES 8192U
#define BEETLE_WSS_READ_CHUNK_BYTES 1024U
#define BEETLE_WSS_FRAME_HEADER_BUFFER_LIMIT (BEETLE_WSS_READ_CHUNK_BYTES + 14U)

typedef struct {
    uint32_t state[5];
    uint64_t total_len;
    uint8_t block[64];
    size_t block_len;
} beetle_wss_sha1_t;

typedef enum {
    BEETLE_WSS_PENDING_NONE = 0,
    BEETLE_WSS_PENDING_EVENT,
    BEETLE_WSS_PENDING_FRAGMENT,
    BEETLE_WSS_PENDING_CONTROL,
} beetle_wss_pending_target_t;

struct beetle_wss_client {
    esp_tls_t *tls;
    int sockfd;
    uint32_t io_timeout_ms;
    uint32_t max_message_bytes;
    bool close_sent;
    bool close_received;
    uint8_t *rx_buf;
    size_t rx_len;
    size_t rx_cap;
    uint8_t *fragment_buf;
    size_t fragment_len;
    size_t fragment_cap;
    uint8_t fragment_opcode;
    bool pending_frame_active;
    uint8_t pending_opcode;
    bool pending_fin;
    beetle_wss_pending_target_t pending_target;
    size_t pending_payload_len;
    size_t pending_payload_read;
    uint8_t *pending_event_buf;
    uint8_t pending_control_payload[125];
    bool has_last_close_code;
    uint16_t last_close_code;
    uint8_t *last_close_reason;
    size_t last_close_reason_len;
};

typedef struct {
    char *host;
    char *host_header;
    char *path;
    int port;
} beetle_wss_url_t;

static void beetle_wss_sha1_transform(beetle_wss_sha1_t *ctx, const uint8_t block[64]) {
    uint32_t w[80];
    uint32_t a;
    uint32_t b;
    uint32_t c;
    uint32_t d;
    uint32_t e;

    for (size_t i = 0; i < 16; ++i) {
        w[i] = ((uint32_t) block[i * 4] << 24) |
               ((uint32_t) block[i * 4 + 1] << 16) |
               ((uint32_t) block[i * 4 + 2] << 8) |
               (uint32_t) block[i * 4 + 3];
    }
    for (size_t i = 16; i < 80; ++i) {
        uint32_t v = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
        w[i] = (v << 1) | (v >> 31);
    }

    a = ctx->state[0];
    b = ctx->state[1];
    c = ctx->state[2];
    d = ctx->state[3];
    e = ctx->state[4];

    for (size_t i = 0; i < 80; ++i) {
        uint32_t f;
        uint32_t k;
        if (i < 20) {
            f = (b & c) | ((~b) & d);
            k = 0x5A827999U;
        } else if (i < 40) {
            f = b ^ c ^ d;
            k = 0x6ED9EBA1U;
        } else if (i < 60) {
            f = (b & c) | (b & d) | (c & d);
            k = 0x8F1BBCDCU;
        } else {
            f = b ^ c ^ d;
            k = 0xCA62C1D6U;
        }

        uint32_t temp = ((a << 5) | (a >> 27)) + f + e + k + w[i];
        e = d;
        d = c;
        c = (b << 30) | (b >> 2);
        b = a;
        a = temp;
    }

    ctx->state[0] += a;
    ctx->state[1] += b;
    ctx->state[2] += c;
    ctx->state[3] += d;
    ctx->state[4] += e;
}

static void beetle_wss_sha1_init(beetle_wss_sha1_t *ctx) {
    memset(ctx, 0, sizeof(*ctx));
    ctx->state[0] = 0x67452301U;
    ctx->state[1] = 0xEFCDAB89U;
    ctx->state[2] = 0x98BADCFEU;
    ctx->state[3] = 0x10325476U;
    ctx->state[4] = 0xC3D2E1F0U;
}

static void beetle_wss_sha1_update(
    beetle_wss_sha1_t *ctx,
    const uint8_t *data,
    size_t len
) {
    if (data == NULL || len == 0) {
        return;
    }

    ctx->total_len += len;

    while (len > 0) {
        size_t to_copy = sizeof(ctx->block) - ctx->block_len;
        if (to_copy > len) {
            to_copy = len;
        }
        memcpy(ctx->block + ctx->block_len, data, to_copy);
        ctx->block_len += to_copy;
        data += to_copy;
        len -= to_copy;

        if (ctx->block_len == sizeof(ctx->block)) {
            beetle_wss_sha1_transform(ctx, ctx->block);
            ctx->block_len = 0;
        }
    }
}

static void beetle_wss_sha1_final(beetle_wss_sha1_t *ctx, uint8_t out[20]) {
    uint64_t bits = ctx->total_len * 8U;
    uint8_t pad = 0x80U;
    uint8_t zero = 0;
    uint8_t len_bytes[8];

    for (size_t i = 0; i < 8; ++i) {
        len_bytes[7 - i] = (uint8_t) ((bits >> (i * 8U)) & 0xFFU);
    }

    beetle_wss_sha1_update(ctx, &pad, 1);
    while (ctx->block_len != 56U) {
        beetle_wss_sha1_update(ctx, &zero, 1);
    }
    beetle_wss_sha1_update(ctx, len_bytes, sizeof(len_bytes));

    for (size_t i = 0; i < 5; ++i) {
        out[i * 4] = (uint8_t) ((ctx->state[i] >> 24) & 0xFFU);
        out[i * 4 + 1] = (uint8_t) ((ctx->state[i] >> 16) & 0xFFU);
        out[i * 4 + 2] = (uint8_t) ((ctx->state[i] >> 8) & 0xFFU);
        out[i * 4 + 3] = (uint8_t) (ctx->state[i] & 0xFFU);
    }
}

static bool beetle_wss_base64_encode(
    const uint8_t *input,
    size_t input_len,
    char *out,
    size_t out_cap
) {
    static const char TABLE[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t out_len = ((input_len + 2U) / 3U) * 4U;
    size_t in_pos = 0;
    size_t out_pos = 0;

    if (out == NULL || out_cap < out_len + 1U) {
        return false;
    }

    while (in_pos < input_len) {
        size_t remaining = input_len - in_pos;
        uint32_t octet_a = input[in_pos++];
        uint32_t octet_b = remaining > 1U ? input[in_pos++] : 0U;
        uint32_t octet_c = remaining > 2U ? input[in_pos++] : 0U;
        uint32_t triple = (octet_a << 16) | (octet_b << 8) | octet_c;

        out[out_pos++] = TABLE[(triple >> 18) & 0x3FU];
        out[out_pos++] = TABLE[(triple >> 12) & 0x3FU];
        out[out_pos++] = remaining > 1U ? TABLE[(triple >> 6) & 0x3FU] : '=';
        out[out_pos++] = remaining > 2U ? TABLE[triple & 0x3FU] : '=';
    }
    out[out_len] = '\0';
    return true;
}

static int64_t beetle_wss_now_ms(void) {
    return esp_timer_get_time() / 1000;
}

static uint32_t beetle_wss_effective_timeout(uint32_t timeout_ms, uint32_t fallback_ms) {
    if (timeout_ms != 0U) {
        return timeout_ms;
    }
    return fallback_ms;
}

static uint32_t beetle_wss_deadline_remaining_ms(int64_t deadline_ms) {
    int64_t now_ms = beetle_wss_now_ms();
    if (deadline_ms <= now_ms) {
        return 0;
    }
    int64_t remaining = deadline_ms - now_ms;
    if (remaining > (int64_t) UINT32_MAX) {
        return UINT32_MAX;
    }
    return (uint32_t) remaining;
}

static void beetle_wss_event_reset(beetle_wss_event_t *event) {
    if (event == NULL) {
        return;
    }
    event->data = NULL;
    event->len = 0;
}

static void beetle_wss_reset_last_close(beetle_wss_client_t *client) {
    if (client == NULL) {
        return;
    }
    client->has_last_close_code = false;
    client->last_close_code = 0U;
    free(client->last_close_reason);
    client->last_close_reason = NULL;
    client->last_close_reason_len = 0U;
}

static beetle_wss_status_t beetle_wss_store_close_payload(
    beetle_wss_client_t *client,
    const uint8_t *payload,
    size_t payload_len
) {
    if (client == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    beetle_wss_reset_last_close(client);

    if (payload == NULL || payload_len == 0U) {
        return BEETLE_WSS_OK;
    }
    if (payload_len >= 2U) {
        client->has_last_close_code = true;
        client->last_close_code = ((uint16_t) payload[0] << 8U) | (uint16_t) payload[1];
    }
    if (payload_len > 2U) {
        size_t reason_len = payload_len - 2U;
        client->last_close_reason = (uint8_t *) malloc(reason_len);
        if (client->last_close_reason == NULL) {
            beetle_wss_reset_last_close(client);
            return BEETLE_WSS_ERR_NOMEM;
        }
        memcpy(client->last_close_reason, payload + 2U, reason_len);
        client->last_close_reason_len = reason_len;
    }
    return BEETLE_WSS_OK;
}

static void beetle_wss_free_url(beetle_wss_url_t *url) {
    if (url == NULL) {
        return;
    }
    free(url->host);
    free(url->host_header);
    free(url->path);
    memset(url, 0, sizeof(*url));
}

static bool beetle_wss_contains_forbidden_nl(const char *value) {
    return value != NULL && strstr(value, "\r\n\r\n") != NULL;
}

static char *beetle_wss_dup_range(const char *start, size_t len) {
    char *value = (char *) malloc(len + 1U);
    if (value == NULL) {
        return NULL;
    }
    memcpy(value, start, len);
    value[len] = '\0';
    return value;
}

static beetle_wss_status_t beetle_wss_parse_url(
    const char *url,
    beetle_wss_url_t *parsed
) {
    const char *scheme = "wss://";
    const char *cursor;
    const char *host_start;
    const char *host_end;
    const char *path_start;
    size_t host_len;
    const char *colon;
    char *port_text = NULL;
    char *port_end = NULL;

    if (url == NULL || parsed == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    memset(parsed, 0, sizeof(*parsed));

    if (strncmp(url, scheme, strlen(scheme)) != 0) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    cursor = url + strlen(scheme);
    host_start = cursor;
    while (*cursor != '\0' && *cursor != '/' && *cursor != '?' && *cursor != '#') {
        ++cursor;
    }
    host_end = cursor;
    host_len = (size_t) (host_end - host_start);
    if (host_len == 0U) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    path_start = cursor;
    if (*path_start == '\0') {
        parsed->path = beetle_wss_dup_range("/", 1U);
    } else if (*path_start == '#') {
        parsed->path = beetle_wss_dup_range("/", 1U);
    } else {
        const char *path_end = path_start;
        while (*path_end != '\0' && *path_end != '#') {
            ++path_end;
        }
        parsed->path = beetle_wss_dup_range(path_start, (size_t) (path_end - path_start));
    }
    if (parsed->path == NULL) {
        beetle_wss_free_url(parsed);
        return BEETLE_WSS_ERR_NOMEM;
    }

    colon = memchr(host_start, ':', host_len);
    if (colon != NULL) {
        parsed->host = beetle_wss_dup_range(host_start, (size_t) (colon - host_start));
        if (parsed->host == NULL) {
            beetle_wss_free_url(parsed);
            return BEETLE_WSS_ERR_NOMEM;
        }
        if (parsed->host[0] == '\0') {
            beetle_wss_free_url(parsed);
            return BEETLE_WSS_ERR_INVALID_ARG;
        }
        port_text = beetle_wss_dup_range(colon + 1, (size_t) (host_end - (colon + 1)));
        if (port_text == NULL) {
            beetle_wss_free_url(parsed);
            return BEETLE_WSS_ERR_NOMEM;
        }
        errno = 0;
        long port = strtol(port_text, &port_end, 10);
        if (errno != 0 || port_end == port_text || *port_end != '\0' || port <= 0 || port > 65535) {
            free(port_text);
            beetle_wss_free_url(parsed);
            return BEETLE_WSS_ERR_INVALID_ARG;
        }
        free(port_text);
        parsed->port = (int) port;
    } else {
        parsed->host = beetle_wss_dup_range(host_start, host_len);
        if (parsed->host == NULL) {
            beetle_wss_free_url(parsed);
            return BEETLE_WSS_ERR_NOMEM;
        }
        parsed->port = BEETLE_WSS_DEFAULT_PORT;
    }

    if (strchr(parsed->host, '[') != NULL || strchr(parsed->host, ']') != NULL) {
        beetle_wss_free_url(parsed);
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    if (parsed->port == BEETLE_WSS_DEFAULT_PORT) {
        parsed->host_header = beetle_wss_dup_range(parsed->host, strlen(parsed->host));
    } else {
        size_t host_header_len = strlen(parsed->host) + 8U;
        parsed->host_header = (char *) malloc(host_header_len);
        if (parsed->host_header != NULL) {
            snprintf(parsed->host_header, host_header_len, "%s:%d", parsed->host, parsed->port);
        }
    }
    if (parsed->host_header == NULL) {
        beetle_wss_free_url(parsed);
        return BEETLE_WSS_ERR_NOMEM;
    }

    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_set_socket_timeout(
    int sockfd,
    int optname,
    uint32_t timeout_ms
) {
    struct timeval tv;

    if (sockfd < 0) {
        return BEETLE_WSS_ERR_STATE;
    }

    tv.tv_sec = timeout_ms / 1000U;
    tv.tv_usec = (suseconds_t) ((timeout_ms % 1000U) * 1000U);
    if (setsockopt(sockfd, SOL_SOCKET, optname, &tv, sizeof(tv)) != 0) {
        return BEETLE_WSS_ERR_IO;
    }
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_ensure_rx_capacity(
    beetle_wss_client_t *client,
    size_t needed
) {
    size_t new_cap;
    uint8_t *new_buf;

    if (client == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (needed <= client->rx_cap) {
        return BEETLE_WSS_OK;
    }

    new_cap = client->rx_cap == 0U ? 2048U : client->rx_cap;
    while (new_cap < needed) {
        if (new_cap > SIZE_MAX / 2U) {
            return BEETLE_WSS_ERR_NOMEM;
        }
        new_cap *= 2U;
    }

    new_buf = (uint8_t *) realloc(client->rx_buf, new_cap);
    if (new_buf == NULL) {
        return BEETLE_WSS_ERR_NOMEM;
    }
    client->rx_buf = new_buf;
    client->rx_cap = new_cap;
    return BEETLE_WSS_OK;
}

static void beetle_wss_consume_rx(beetle_wss_client_t *client, size_t len) {
    if (client == NULL || len == 0U) {
        return;
    }
    if (len >= client->rx_len) {
        client->rx_len = 0U;
        return;
    }
    memmove(client->rx_buf, client->rx_buf + len, client->rx_len - len);
    client->rx_len -= len;
}

static ssize_t beetle_wss_find_header_end(const uint8_t *buf, size_t len) {
    if (buf == NULL || len < 4U) {
        return -1;
    }
    for (size_t i = 0; i + 3U < len; ++i) {
        if (buf[i] == '\r' && buf[i + 1U] == '\n' &&
            buf[i + 2U] == '\r' && buf[i + 3U] == '\n') {
            return (ssize_t) (i + 4U);
        }
    }
    return -1;
}

static bool beetle_wss_is_retryable_tls_read(int ret) {
    return ret == ESP_TLS_ERR_SSL_WANT_READ || ret == ESP_TLS_ERR_SSL_WANT_WRITE;
}

static beetle_wss_status_t beetle_wss_fill_rx(
    beetle_wss_client_t *client,
    int64_t deadline_ms
) {
    beetle_wss_status_t status;
    uint32_t timeout_ms;
    int ret;

    if (client == NULL || client->tls == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    timeout_ms = beetle_wss_deadline_remaining_ms(deadline_ms);
    if (timeout_ms == 0U) {
        return BEETLE_WSS_TIMEOUT;
    }

    status = beetle_wss_set_socket_timeout(client->sockfd, SO_RCVTIMEO, timeout_ms);
    if (status != BEETLE_WSS_OK) {
        return status;
    }
    if (client->rx_len > SIZE_MAX - BEETLE_WSS_READ_CHUNK_BYTES) {
        return BEETLE_WSS_ERR_NOMEM;
    }
    status = beetle_wss_ensure_rx_capacity(client, client->rx_len + BEETLE_WSS_READ_CHUNK_BYTES);
    if (status != BEETLE_WSS_OK) {
        return status;
    }

    ret = (int) esp_tls_conn_read(
        client->tls,
        client->rx_buf + client->rx_len,
        BEETLE_WSS_READ_CHUNK_BYTES
    );
    if (ret > 0) {
        client->rx_len += (size_t) ret;
        return BEETLE_WSS_OK;
    }
    if (ret == 0) {
        return client->close_received ? BEETLE_WSS_CLOSED : BEETLE_WSS_DISCONNECTED;
    }
    if (beetle_wss_is_retryable_tls_read(ret)) {
        return beetle_wss_deadline_remaining_ms(deadline_ms) == 0U
            ? BEETLE_WSS_TIMEOUT
            : BEETLE_WSS_TIMEOUT;
    }
    return BEETLE_WSS_DISCONNECTED;
}

static beetle_wss_status_t beetle_wss_read_into(
    beetle_wss_client_t *client,
    uint8_t *target,
    size_t target_len,
    int64_t deadline_ms,
    size_t *out_read
) {
    beetle_wss_status_t status;
    uint32_t timeout_ms;
    size_t read_len;
    int ret;

    if (client == NULL || client->tls == NULL || target == NULL || out_read == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    *out_read = 0U;
    if (target_len == 0U) {
        return BEETLE_WSS_OK;
    }

    timeout_ms = beetle_wss_deadline_remaining_ms(deadline_ms);
    if (timeout_ms == 0U) {
        return BEETLE_WSS_TIMEOUT;
    }
    status = beetle_wss_set_socket_timeout(client->sockfd, SO_RCVTIMEO, timeout_ms);
    if (status != BEETLE_WSS_OK) {
        return status;
    }

    read_len = target_len > BEETLE_WSS_READ_CHUNK_BYTES
        ? BEETLE_WSS_READ_CHUNK_BYTES
        : target_len;
    ret = (int) esp_tls_conn_read(client->tls, target, read_len);
    if (ret > 0) {
        *out_read = (size_t) ret;
        return BEETLE_WSS_OK;
    }
    if (ret == 0) {
        return client->close_received ? BEETLE_WSS_CLOSED : BEETLE_WSS_DISCONNECTED;
    }
    if (beetle_wss_is_retryable_tls_read(ret)) {
        return BEETLE_WSS_TIMEOUT;
    }
    return BEETLE_WSS_DISCONNECTED;
}

static beetle_wss_status_t beetle_wss_ensure_rx_bytes(
    beetle_wss_client_t *client,
    size_t needed,
    int64_t deadline_ms
) {
    beetle_wss_status_t status;

    while (client->rx_len < needed) {
        status = beetle_wss_fill_rx(client, deadline_ms);
        if (status != BEETLE_WSS_OK) {
            return status;
        }
    }
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_ensure_rx_bytes_limited(
    beetle_wss_client_t *client,
    size_t needed,
    size_t limit,
    int64_t deadline_ms
) {
    if (needed > limit) {
        return BEETLE_WSS_ERR_PROTOCOL;
    }
    while (client->rx_len < needed) {
        beetle_wss_status_t status;
        if (client->rx_len >= limit) {
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        status = beetle_wss_fill_rx(client, deadline_ms);
        if (status != BEETLE_WSS_OK) {
            return status;
        }
    }
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_write_all(
    beetle_wss_client_t *client,
    const uint8_t *data,
    size_t len,
    int64_t deadline_ms
) {
    size_t written = 0U;

    if (client == NULL || client->tls == NULL || (len > 0U && data == NULL)) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    while (written < len) {
        beetle_wss_status_t status;
        uint32_t timeout_ms = beetle_wss_deadline_remaining_ms(deadline_ms);
        int ret;

        if (timeout_ms == 0U) {
            return BEETLE_WSS_TIMEOUT;
        }
        status = beetle_wss_set_socket_timeout(client->sockfd, SO_SNDTIMEO, timeout_ms);
        if (status != BEETLE_WSS_OK) {
            return status;
        }

        ret = (int) esp_tls_conn_write(client->tls, data + written, len - written);
        if (ret > 0) {
            written += (size_t) ret;
            continue;
        }
        if (ret == 0) {
            return client->close_received ? BEETLE_WSS_CLOSED : BEETLE_WSS_DISCONNECTED;
        }
        if (beetle_wss_is_retryable_tls_read(ret)) {
            return BEETLE_WSS_TIMEOUT;
        }
        return BEETLE_WSS_DISCONNECTED;
    }

    return BEETLE_WSS_OK;
}

static char *beetle_wss_trim(char *value) {
    char *end;

    while (*value != '\0' && isspace((unsigned char) *value)) {
        ++value;
    }
    end = value + strlen(value);
    while (end > value && isspace((unsigned char) end[-1])) {
        --end;
    }
    *end = '\0';
    return value;
}

static bool beetle_wss_connection_has_upgrade_token(const char *value) {
    const char *cursor = value;

    while (cursor != NULL && *cursor != '\0') {
        while (*cursor == ' ' || *cursor == '\t' || *cursor == ',') {
            ++cursor;
        }
        const char *token_end = cursor;
        while (*token_end != '\0' && *token_end != ',') {
            ++token_end;
        }
        size_t token_len = (size_t) (token_end - cursor);
        while (token_len > 0U &&
               isspace((unsigned char) cursor[token_len - 1U])) {
            --token_len;
        }
        if (token_len == 7U && strncasecmp(cursor, "upgrade", 7U) == 0) {
            return true;
        }
        cursor = *token_end == '\0' ? NULL : token_end + 1;
    }
    return false;
}

static bool beetle_wss_compute_accept(
    const char *sec_key,
    char out[29]
) {
    beetle_wss_sha1_t sha1;
    uint8_t digest[20];

    beetle_wss_sha1_init(&sha1);
    beetle_wss_sha1_update(&sha1, (const uint8_t *) sec_key, strlen(sec_key));
    beetle_wss_sha1_update(
        &sha1,
        (const uint8_t *) BEETLE_WSS_GUID,
        strlen(BEETLE_WSS_GUID)
    );
    beetle_wss_sha1_final(&sha1, digest);
    return beetle_wss_base64_encode(digest, sizeof(digest), out, 29U);
}

static beetle_wss_status_t beetle_wss_verify_handshake_response(
    beetle_wss_client_t *client,
    const char *expected_accept,
    int64_t deadline_ms
) {
    beetle_wss_status_t status;
    ssize_t header_end = -1;

    while (true) {
        if (client->rx_len > BEETLE_WSS_HANDSHAKE_MAX_HEADER_BYTES) {
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        header_end = beetle_wss_find_header_end(client->rx_buf, client->rx_len);
        if (header_end >= 0) {
            break;
        }
        status = beetle_wss_fill_rx(client, deadline_ms);
        if (status != BEETLE_WSS_OK) {
            return status;
        }
    }

    char *headers = (char *) malloc((size_t) header_end + 1U);
    if (headers == NULL) {
        return BEETLE_WSS_ERR_NOMEM;
    }
    memcpy(headers, client->rx_buf, (size_t) header_end);
    headers[header_end] = '\0';

    bool saw_upgrade = false;
    bool saw_connection = false;
    bool saw_accept = false;
    beetle_wss_status_t result = BEETLE_WSS_ERR_PROTOCOL;
    const char *status_line = NULL;

    char *saveptr = NULL;
    char *line = strtok_r(headers, "\r\n", &saveptr);
    if (line == NULL) {
        goto cleanup;
    }
    status_line = line;
    if (strncmp(line, "HTTP/1.1 101", 12U) != 0 && strncmp(line, "HTTP/1.0 101", 12U) != 0) {
        goto cleanup;
    }

    while ((line = strtok_r(NULL, "\r\n", &saveptr)) != NULL) {
        char *colon = strchr(line, ':');
        char *value;
        if (colon == NULL) {
            continue;
        }
        *colon = '\0';
        value = beetle_wss_trim(colon + 1);
        if (strcasecmp(line, "Upgrade") == 0) {
            if (strcasecmp(value, "websocket") == 0) {
                saw_upgrade = true;
            }
        } else if (strcasecmp(line, "Connection") == 0) {
            if (beetle_wss_connection_has_upgrade_token(value)) {
                saw_connection = true;
            }
        } else if (strcasecmp(line, "Sec-WebSocket-Accept") == 0) {
            if (strcmp(value, expected_accept) == 0) {
                saw_accept = true;
            }
        }
    }

    if (saw_upgrade && saw_connection && saw_accept) {
        beetle_wss_consume_rx(client, (size_t) header_end);
        result = BEETLE_WSS_OK;
    }

cleanup:
    if (result != BEETLE_WSS_OK) {
        ESP_LOGW(
            BEETLE_WSS_LOG_TAG,
            "websocket handshake rejected status=\"%s\" upgrade=%d connection=%d accept=%d",
            status_line == NULL ? "(missing)" : status_line,
            saw_upgrade ? 1 : 0,
            saw_connection ? 1 : 0,
            saw_accept ? 1 : 0
        );
    }
    free(headers);
    return result;
}

static beetle_wss_status_t beetle_wss_send_http_upgrade(
    beetle_wss_client_t *client,
    const beetle_wss_url_t *url,
    const char *extra_headers,
    int64_t deadline_ms
) {
    uint8_t random_key[16];
    char sec_key[25];
    char expected_accept[29];
    size_t request_cap;
    char *request = NULL;
    size_t request_len;
    beetle_wss_status_t status;

    if (client == NULL || url == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (extra_headers != NULL && beetle_wss_contains_forbidden_nl(extra_headers)) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }

    esp_fill_random(random_key, sizeof(random_key));
    if (!beetle_wss_base64_encode(random_key, sizeof(random_key), sec_key, sizeof(sec_key))) {
        return BEETLE_WSS_ERR_PROTOCOL;
    }
    if (!beetle_wss_compute_accept(sec_key, expected_accept)) {
        return BEETLE_WSS_ERR_PROTOCOL;
    }

    request_cap = strlen(url->path) + strlen(url->host_header) +
        (extra_headers == NULL ? 0U : strlen(extra_headers)) + 256U;
    request = (char *) malloc(request_cap);
    if (request == NULL) {
        return BEETLE_WSS_ERR_NOMEM;
    }

    request_len = (size_t) snprintf(
        request,
        request_cap,
        "GET %s HTTP/1.1\r\n"
        "Host: %s\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "Sec-WebSocket-Key: %s\r\n"
        "User-Agent: beetle-wss/0.1\r\n",
        url->path,
        url->host_header,
        sec_key
    );
    if (request_len >= request_cap) {
        free(request);
        return BEETLE_WSS_ERR_NOMEM;
    }

    if (extra_headers != NULL && extra_headers[0] != '\0') {
        size_t extra_len = strlen(extra_headers);
        if (request_len + extra_len + 4U >= request_cap) {
            free(request);
            return BEETLE_WSS_ERR_NOMEM;
        }
        memcpy(request + request_len, extra_headers, extra_len);
        request_len += extra_len;
        if (request_len < 2U ||
            request[request_len - 2U] != '\r' ||
            request[request_len - 1U] != '\n') {
            request[request_len++] = '\r';
            request[request_len++] = '\n';
        }
    }
    request[request_len++] = '\r';
    request[request_len++] = '\n';

    status = beetle_wss_write_all(client, (const uint8_t *) request, request_len, deadline_ms);
    free(request);
    if (status != BEETLE_WSS_OK) {
        return status;
    }
    return beetle_wss_verify_handshake_response(client, expected_accept, deadline_ms);
}

static beetle_wss_status_t beetle_wss_reserve_fragment_append(
    beetle_wss_client_t *client,
    size_t payload_len
) {
    uint8_t *new_buf;
    size_t needed;

    if (client == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (payload_len == 0U) {
        return BEETLE_WSS_OK;
    }
    if (client->fragment_len > SIZE_MAX - payload_len) {
        return BEETLE_WSS_ERR_NOMEM;
    }

    needed = client->fragment_len + payload_len;
    if (needed > client->max_message_bytes) {
        return BEETLE_WSS_ERR_PROTOCOL;
    }
    if (needed > client->fragment_cap) {
        size_t new_cap = client->fragment_cap == 0U ? payload_len : client->fragment_cap;
        while (new_cap < needed) {
            if (new_cap > SIZE_MAX / 2U) {
                return BEETLE_WSS_ERR_NOMEM;
            }
            new_cap *= 2U;
        }
        new_buf = (uint8_t *) realloc(client->fragment_buf, new_cap);
        if (new_buf == NULL) {
            return BEETLE_WSS_ERR_NOMEM;
        }
        client->fragment_buf = new_buf;
        client->fragment_cap = new_cap;
    }

    return BEETLE_WSS_OK;
}

static void beetle_wss_note_fragment_bytes_read(
    beetle_wss_client_t *client,
    size_t bytes_read
) {
    if (client == NULL || bytes_read == 0U) {
        return;
    }
    client->fragment_len += bytes_read;
}

static void beetle_wss_reset_fragment(beetle_wss_client_t *client) {
    if (client == NULL) {
        return;
    }
    free(client->fragment_buf);
    client->fragment_buf = NULL;
    client->fragment_len = 0U;
    client->fragment_cap = 0U;
    client->fragment_opcode = 0U;
}

static void beetle_wss_clear_pending_frame(beetle_wss_client_t *client) {
    if (client == NULL) {
        return;
    }
    free(client->pending_event_buf);
    client->pending_event_buf = NULL;
    client->pending_frame_active = false;
    client->pending_opcode = 0U;
    client->pending_fin = false;
    client->pending_target = BEETLE_WSS_PENDING_NONE;
    client->pending_payload_len = 0U;
    client->pending_payload_read = 0U;
}

static void beetle_wss_abort_pending_frame(beetle_wss_client_t *client) {
    if (client == NULL) {
        return;
    }
    if (client->pending_target == BEETLE_WSS_PENDING_FRAGMENT) {
        beetle_wss_reset_fragment(client);
    }
    beetle_wss_clear_pending_frame(client);
}

static uint8_t *beetle_wss_pending_write_ptr(beetle_wss_client_t *client) {
    if (client == NULL || !client->pending_frame_active) {
        return NULL;
    }
    switch (client->pending_target) {
        case BEETLE_WSS_PENDING_EVENT:
            return client->pending_event_buf + client->pending_payload_read;
        case BEETLE_WSS_PENDING_FRAGMENT:
            return client->fragment_buf + client->fragment_len;
        case BEETLE_WSS_PENDING_CONTROL:
            return client->pending_control_payload + client->pending_payload_read;
        case BEETLE_WSS_PENDING_NONE:
        default:
            return NULL;
    }
}

static void beetle_wss_note_pending_bytes_read(
    beetle_wss_client_t *client,
    size_t bytes_read
) {
    if (client == NULL || bytes_read == 0U) {
        return;
    }
    if (client->pending_target == BEETLE_WSS_PENDING_FRAGMENT) {
        beetle_wss_note_fragment_bytes_read(client, bytes_read);
    }
    client->pending_payload_read += bytes_read;
}

static beetle_wss_status_t beetle_wss_read_pending_payload(
    beetle_wss_client_t *client,
    int64_t deadline_ms
) {
    if (client == NULL || !client->pending_frame_active) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    while (client->pending_payload_read < client->pending_payload_len) {
        size_t remaining = client->pending_payload_len - client->pending_payload_read;
        uint8_t *target = beetle_wss_pending_write_ptr(client);
        if (target == NULL) {
            return BEETLE_WSS_ERR_STATE;
        }
        if (client->rx_len > 0U) {
            size_t from_rx = client->rx_len < remaining ? client->rx_len : remaining;
            memcpy(target, client->rx_buf, from_rx);
            beetle_wss_consume_rx(client, from_rx);
            beetle_wss_note_pending_bytes_read(client, from_rx);
            continue;
        }

        size_t bytes_read = 0U;
        beetle_wss_status_t status =
            beetle_wss_read_into(client, target, remaining, deadline_ms, &bytes_read);
        if (status != BEETLE_WSS_OK) {
            return status;
        }
        beetle_wss_note_pending_bytes_read(client, bytes_read);
    }
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_begin_pending_frame(
    beetle_wss_client_t *client,
    uint8_t opcode,
    bool fin,
    size_t payload_len
) {
    if (client == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    client->pending_opcode = opcode;
    client->pending_fin = fin;
    client->pending_payload_len = payload_len;
    client->pending_payload_read = 0U;
    client->pending_event_buf = NULL;

    if (opcode == 0x00U) {
        if (client->fragment_opcode == 0U) {
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        beetle_wss_status_t status = beetle_wss_reserve_fragment_append(client, payload_len);
        if (status != BEETLE_WSS_OK) {
            return status;
        }
        client->pending_target = BEETLE_WSS_PENDING_FRAGMENT;
    } else if (opcode == 0x01U || opcode == 0x02U) {
        if (client->fragment_opcode != 0U) {
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        if (fin) {
            if (payload_len > 0U) {
                client->pending_event_buf = (uint8_t *) malloc(payload_len);
                if (client->pending_event_buf == NULL) {
                    return BEETLE_WSS_ERR_NOMEM;
                }
            }
            client->pending_target = BEETLE_WSS_PENDING_EVENT;
        } else {
            beetle_wss_status_t status = beetle_wss_reserve_fragment_append(client, payload_len);
            if (status != BEETLE_WSS_OK) {
                return status;
            }
            client->fragment_opcode = opcode;
            client->pending_target = BEETLE_WSS_PENDING_FRAGMENT;
        }
    } else if ((opcode & 0x08U) != 0U) {
        client->pending_target = BEETLE_WSS_PENDING_CONTROL;
    } else {
        return BEETLE_WSS_ERR_PROTOCOL;
    }

    client->pending_frame_active = true;
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_send_frame(
    beetle_wss_client_t *client,
    uint8_t opcode,
    const uint8_t *payload,
    size_t payload_len,
    uint32_t timeout_ms
) {
    uint8_t header[14];
    uint8_t mask[4];
    size_t header_len = 0U;
    int64_t deadline_ms;
    beetle_wss_status_t status;

    if (client == NULL || client->tls == NULL || (payload_len > 0U && payload == NULL)) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (payload_len > client->max_message_bytes) {
        return BEETLE_WSS_ERR_PROTOCOL;
    }

    header[header_len++] = 0x80U | (opcode & 0x0FU);
    if (payload_len <= 125U) {
        header[header_len++] = 0x80U | (uint8_t) payload_len;
    } else if (payload_len <= 0xFFFFU) {
        header[header_len++] = 0x80U | 126U;
        header[header_len++] = (uint8_t) ((payload_len >> 8U) & 0xFFU);
        header[header_len++] = (uint8_t) (payload_len & 0xFFU);
    } else {
        uint64_t extended = (uint64_t) payload_len;
        header[header_len++] = 0x80U | 127U;
        for (int i = 7; i >= 0; --i) {
            header[header_len++] = (uint8_t) ((extended >> (i * 8U)) & 0xFFU);
        }
    }

    esp_fill_random(mask, sizeof(mask));
    memcpy(header + header_len, mask, sizeof(mask));
    header_len += sizeof(mask);

    deadline_ms = beetle_wss_now_ms() +
        (int64_t) beetle_wss_effective_timeout(timeout_ms, client->io_timeout_ms);
    status = beetle_wss_write_all(client, header, header_len, deadline_ms);
    if (status != BEETLE_WSS_OK) {
        return status;
    }

    if (payload_len > 0U) {
        uint8_t chunk[512];
        size_t offset = 0U;
        while (offset < payload_len) {
            size_t chunk_len = payload_len - offset;
            if (chunk_len > sizeof(chunk)) {
                chunk_len = sizeof(chunk);
            }
            for (size_t i = 0; i < chunk_len; ++i) {
                chunk[i] = payload[offset + i] ^ mask[(offset + i) % sizeof(mask)];
            }
            status = beetle_wss_write_all(client, chunk, chunk_len, deadline_ms);
            if (status != BEETLE_WSS_OK) {
                return status;
            }
            offset += chunk_len;
        }
    }

    if (opcode == 0x08U) {
        client->close_sent = true;
    }
    return BEETLE_WSS_OK;
}

static beetle_wss_status_t beetle_wss_send_close_code(
    beetle_wss_client_t *client,
    uint16_t code,
    uint32_t timeout_ms
) {
    uint8_t payload[2];
    payload[0] = (uint8_t) ((code >> 8U) & 0xFFU);
    payload[1] = (uint8_t) (code & 0xFFU);
    return beetle_wss_send_frame(client, 0x08U, payload, sizeof(payload), timeout_ms);
}

beetle_wss_status_t beetle_wss_connect(
    const beetle_wss_config_t *config,
    beetle_wss_client_t **out_client
) {
    beetle_wss_url_t parsed_url;
    beetle_wss_client_t *client = NULL;
    esp_tls_cfg_t tls_cfg;
    beetle_wss_status_t status;
    uint32_t connect_timeout_ms;
    uint32_t io_timeout_ms;
    uint32_t max_message_bytes;
    int ret;

    if (config == NULL || out_client == NULL || config->url == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    *out_client = NULL;

    connect_timeout_ms = beetle_wss_effective_timeout(
        config->connect_timeout_ms,
        BEETLE_WSS_DEFAULT_CONNECT_TIMEOUT_MS
    );
    io_timeout_ms = beetle_wss_effective_timeout(
        config->io_timeout_ms,
        BEETLE_WSS_DEFAULT_IO_TIMEOUT_MS
    );
    max_message_bytes = beetle_wss_effective_timeout(
        config->max_message_bytes,
        BEETLE_WSS_DEFAULT_MAX_MESSAGE_BYTES
    );

    status = beetle_wss_parse_url(config->url, &parsed_url);
    if (status != BEETLE_WSS_OK) {
        return status;
    }

    client = (beetle_wss_client_t *) calloc(1, sizeof(*client));
    if (client == NULL) {
        beetle_wss_free_url(&parsed_url);
        return BEETLE_WSS_ERR_NOMEM;
    }
    client->sockfd = -1;
    client->io_timeout_ms = io_timeout_ms;
    client->max_message_bytes = max_message_bytes;

    client->tls = esp_tls_init();
    if (client->tls == NULL) {
        status = BEETLE_WSS_ERR_TLS;
        goto fail;
    }

    memset(&tls_cfg, 0, sizeof(tls_cfg));
    tls_cfg.timeout_ms = (int) connect_timeout_ms;
    tls_cfg.common_name = parsed_url.host;
    tls_cfg.skip_common_name = false;
    tls_cfg.use_global_ca_store = false;
    tls_cfg.crt_bundle_attach = esp_crt_bundle_attach;

    ret = esp_tls_conn_new_sync(
        parsed_url.host,
        (int) strlen(parsed_url.host),
        parsed_url.port,
        &tls_cfg,
        client->tls
    );
    if (ret != 1) {
        status = BEETLE_WSS_ERR_TLS;
        goto fail;
    }

    if (esp_tls_get_conn_sockfd(client->tls, &client->sockfd) != ESP_OK || client->sockfd < 0) {
        status = BEETLE_WSS_ERR_TLS;
        goto fail;
    }

    status = beetle_wss_send_http_upgrade(
        client,
        &parsed_url,
        config->extra_headers,
        beetle_wss_now_ms() + (int64_t) connect_timeout_ms
    );
    if (status != BEETLE_WSS_OK) {
        goto fail;
    }

    beetle_wss_free_url(&parsed_url);
    *out_client = client;
    return BEETLE_WSS_OK;

fail:
    beetle_wss_free_url(&parsed_url);
    beetle_wss_destroy(client);
    return status;
}

beetle_wss_status_t beetle_wss_send_binary(
    beetle_wss_client_t *client,
    const uint8_t *data,
    size_t len,
    uint32_t timeout_ms
) {
    if (client == NULL || (len > 0U && data == NULL)) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (client->close_sent || client->close_received) {
        return BEETLE_WSS_CLOSED;
    }
    return beetle_wss_send_frame(client, 0x02U, data, len, timeout_ms);
}

beetle_wss_status_t beetle_wss_send_text(
    beetle_wss_client_t *client,
    const char *text,
    size_t len,
    uint32_t timeout_ms
) {
    if (client == NULL || (len > 0U && text == NULL)) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    if (client->close_sent || client->close_received) {
        return BEETLE_WSS_CLOSED;
    }
    return beetle_wss_send_frame(client, 0x01U, (const uint8_t *) text, len, timeout_ms);
}

beetle_wss_status_t beetle_wss_recv(
    beetle_wss_client_t *client,
    uint32_t timeout_ms,
    beetle_wss_event_t *out_event
) {
    beetle_wss_status_t status;
    int64_t deadline_ms;

    if (client == NULL || out_event == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    beetle_wss_event_reset(out_event);
    if (client->close_received) {
        return BEETLE_WSS_CLOSED;
    }

    deadline_ms = beetle_wss_now_ms() + (int64_t) timeout_ms;
    while (true) {
        uint8_t first;
        uint8_t second;
        uint8_t opcode;
        bool fin;
        bool masked;
        size_t header_len = 2U;
        uint64_t payload_len = 0U;
        size_t payload_size;

        if (client->pending_frame_active) {
            status = beetle_wss_read_pending_payload(client, deadline_ms);
            if (status != BEETLE_WSS_OK) {
                if (status != BEETLE_WSS_TIMEOUT) {
                    beetle_wss_abort_pending_frame(client);
                }
                return status;
            }

            opcode = client->pending_opcode;
            payload_size = client->pending_payload_len;
            if (client->pending_target == BEETLE_WSS_PENDING_EVENT) {
                out_event->data = client->pending_event_buf;
                out_event->len = payload_size;
                client->pending_event_buf = NULL;
                beetle_wss_clear_pending_frame(client);
                return BEETLE_WSS_OK;
            }
            if (client->pending_target == BEETLE_WSS_PENDING_FRAGMENT) {
                if (client->pending_fin) {
                    out_event->data = client->fragment_buf;
                    out_event->len = client->fragment_len;
                    client->fragment_buf = NULL;
                    client->fragment_len = 0U;
                    client->fragment_cap = 0U;
                    client->fragment_opcode = 0U;
                    beetle_wss_clear_pending_frame(client);
                    return BEETLE_WSS_OK;
                }
                beetle_wss_clear_pending_frame(client);
                continue;
            }
            if (client->pending_target != BEETLE_WSS_PENDING_CONTROL) {
                beetle_wss_clear_pending_frame(client);
                return BEETLE_WSS_ERR_STATE;
            }

            if (opcode == 0x08U) {
                status = beetle_wss_store_close_payload(
                    client,
                    client->pending_control_payload,
                    payload_size
                );
                if (status != BEETLE_WSS_OK) {
                    beetle_wss_clear_pending_frame(client);
                    return status;
                }
                if (!client->close_sent) {
                    (void) beetle_wss_send_frame(
                        client,
                        0x08U,
                        client->pending_control_payload,
                        payload_size,
                        timeout_ms
                    );
                }
                client->close_received = true;
                beetle_wss_reset_fragment(client);
                beetle_wss_clear_pending_frame(client);
                return BEETLE_WSS_CLOSED;
            }
            if (opcode == 0x09U) {
                status = beetle_wss_send_frame(
                    client,
                    0x0AU,
                    client->pending_control_payload,
                    payload_size,
                    timeout_ms
                );
                beetle_wss_clear_pending_frame(client);
                if (status != BEETLE_WSS_OK) {
                    return status;
                }
                continue;
            }
            if (opcode == 0x0AU) {
                beetle_wss_clear_pending_frame(client);
                continue;
            }
            beetle_wss_clear_pending_frame(client);
            (void) beetle_wss_send_close_code(client, 1002U, timeout_ms);
            return BEETLE_WSS_ERR_PROTOCOL;
        }

        status = beetle_wss_ensure_rx_bytes_limited(
            client,
            2U,
            BEETLE_WSS_FRAME_HEADER_BUFFER_LIMIT,
            deadline_ms
        );
        if (status != BEETLE_WSS_OK) {
            return status;
        }

        first = client->rx_buf[0];
        second = client->rx_buf[1];
        fin = (first & 0x80U) != 0U;
        opcode = first & 0x0FU;
        masked = (second & 0x80U) != 0U;
        payload_len = (uint64_t) (second & 0x7FU);

        if ((first & 0x70U) != 0U || masked) {
            (void) beetle_wss_send_close_code(client, 1002U, timeout_ms);
            return BEETLE_WSS_ERR_PROTOCOL;
        }

        if (payload_len == 126U) {
            status = beetle_wss_ensure_rx_bytes_limited(
                client,
                header_len + 2U,
                BEETLE_WSS_FRAME_HEADER_BUFFER_LIMIT,
                deadline_ms
            );
            if (status != BEETLE_WSS_OK) {
                return status;
            }
            payload_len = ((uint64_t) client->rx_buf[2] << 8U) |
                          (uint64_t) client->rx_buf[3];
            header_len += 2U;
        } else if (payload_len == 127U) {
            status = beetle_wss_ensure_rx_bytes_limited(
                client,
                header_len + 8U,
                BEETLE_WSS_FRAME_HEADER_BUFFER_LIMIT,
                deadline_ms
            );
            if (status != BEETLE_WSS_OK) {
                return status;
            }
            payload_len = 0U;
            for (size_t i = 0; i < 8U; ++i) {
                payload_len = (payload_len << 8U) | (uint64_t) client->rx_buf[2U + i];
            }
            header_len += 8U;
        }

        if (payload_len > (uint64_t) SIZE_MAX) {
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        payload_size = (size_t) payload_len;
        if (payload_len > client->max_message_bytes) {
            (void) beetle_wss_send_close_code(client, 1009U, timeout_ms);
            return BEETLE_WSS_ERR_PROTOCOL;
        }
        if ((opcode & 0x08U) != 0U) {
            if (!fin || payload_len > 125U) {
                (void) beetle_wss_send_close_code(client, 1002U, timeout_ms);
                return BEETLE_WSS_ERR_PROTOCOL;
            }
        }

        if ((opcode == 0x00U || ((opcode == 0x01U || opcode == 0x02U) && !fin)) &&
            client->fragment_len > (size_t) client->max_message_bytes - payload_size) {
            (void) beetle_wss_send_close_code(client, 1009U, timeout_ms);
            return BEETLE_WSS_ERR_PROTOCOL;
        }

        status = beetle_wss_begin_pending_frame(client, opcode, fin, payload_size);
        if (status != BEETLE_WSS_OK) {
            (void) beetle_wss_send_close_code(
                client,
                status == BEETLE_WSS_ERR_PROTOCOL ? 1002U : 1011U,
                timeout_ms
            );
            beetle_wss_abort_pending_frame(client);
            return status;
        }
        beetle_wss_consume_rx(client, header_len);
    }
}

bool beetle_wss_get_last_close_code(
    beetle_wss_client_t *client,
    uint16_t *out_code
) {
    if (client == NULL || out_code == NULL || !client->has_last_close_code) {
        return false;
    }
    *out_code = client->last_close_code;
    return true;
}

beetle_wss_status_t beetle_wss_copy_last_close_reason(
    beetle_wss_client_t *client,
    beetle_wss_event_t *out_event
) {
    if (client == NULL || out_event == NULL) {
        return BEETLE_WSS_ERR_INVALID_ARG;
    }
    beetle_wss_event_reset(out_event);
    if (client->last_close_reason == NULL || client->last_close_reason_len == 0U) {
        return BEETLE_WSS_OK;
    }
    out_event->data = (uint8_t *) malloc(client->last_close_reason_len);
    if (out_event->data == NULL) {
        return BEETLE_WSS_ERR_NOMEM;
    }
    memcpy(out_event->data, client->last_close_reason, client->last_close_reason_len);
    out_event->len = client->last_close_reason_len;
    return BEETLE_WSS_OK;
}

void beetle_wss_free_event(beetle_wss_event_t *event) {
    if (event == NULL) {
        return;
    }
    free(event->data);
    beetle_wss_event_reset(event);
}

void beetle_wss_close(beetle_wss_client_t *client, uint32_t timeout_ms) {
    if (client == NULL || client->tls == NULL || client->close_sent || client->close_received) {
        return;
    }
    (void) beetle_wss_send_frame(client, 0x08U, NULL, 0U, timeout_ms);
}

void beetle_wss_destroy(beetle_wss_client_t *client) {
    if (client == NULL) {
        return;
    }
    if (client->tls != NULL) {
        esp_tls_conn_destroy(client->tls);
        client->tls = NULL;
    }
    beetle_wss_clear_pending_frame(client);
    free(client->rx_buf);
    beetle_wss_reset_last_close(client);
    beetle_wss_reset_fragment(client);
    free(client);
}
