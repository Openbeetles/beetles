/**
 * @file beetle_wss.h
 * @brief beetle-specific minimal synchronous WSS client for ESP.
 *
 * This is intentionally not a general-purpose WebSocket library.
 * It only implements the subset beetle needs:
 * - secure client mode only (`wss://`)
 * - custom request headers
 * - text / binary send
 * - complete text / binary message receive
 * - ping / pong / close handling
 * - bounded buffering and explicit lifetime
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct beetle_wss_client beetle_wss_client_t;

typedef enum {
    BEETLE_WSS_OK = 0,
    BEETLE_WSS_TIMEOUT = 1,
    BEETLE_WSS_CLOSED = 2,
    BEETLE_WSS_DISCONNECTED = 3,
    BEETLE_WSS_ERR_INVALID_ARG = -1,
    BEETLE_WSS_ERR_NOMEM = -2,
    BEETLE_WSS_ERR_TLS = -3,
    BEETLE_WSS_ERR_IO = -4,
    BEETLE_WSS_ERR_PROTOCOL = -5,
    BEETLE_WSS_ERR_STATE = -6,
} beetle_wss_status_t;

typedef struct {
    const char *url;
    const char *extra_headers;
    uint32_t connect_timeout_ms;
    uint32_t io_timeout_ms;
    uint32_t max_message_bytes;
} beetle_wss_config_t;

typedef struct {
    uint8_t *data;
    size_t len;
} beetle_wss_event_t;

beetle_wss_status_t beetle_wss_connect(
    const beetle_wss_config_t *config,
    beetle_wss_client_t **out_client);

beetle_wss_status_t beetle_wss_send_binary(
    beetle_wss_client_t *client,
    const uint8_t *data,
    size_t len,
    uint32_t timeout_ms);

beetle_wss_status_t beetle_wss_send_text(
    beetle_wss_client_t *client,
    const char *text,
    size_t len,
    uint32_t timeout_ms);

beetle_wss_status_t beetle_wss_recv(
    beetle_wss_client_t *client,
    uint32_t timeout_ms,
    beetle_wss_event_t *out_event);

bool beetle_wss_get_last_close_code(
    beetle_wss_client_t *client,
    uint16_t *out_code);

beetle_wss_status_t beetle_wss_copy_last_close_reason(
    beetle_wss_client_t *client,
    beetle_wss_event_t *out_event);

void beetle_wss_free_event(beetle_wss_event_t *event);

void beetle_wss_close(beetle_wss_client_t *client, uint32_t timeout_ms);

void beetle_wss_destroy(beetle_wss_client_t *client);

#ifdef __cplusplus
}
#endif
