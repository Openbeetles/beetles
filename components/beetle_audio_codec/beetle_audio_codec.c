#include "beetle_audio_codec.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "driver/gpio.h"
#include "driver/i2c_master.h"
#include "driver/i2s_std.h"
#include "driver/i2s_tdm.h"
#include "esp_codec_dev.h"
#include "esp_codec_dev_defaults.h"
#include "esp_err.h"
#include "esp_log.h"

#define BEETLE_AUDIO_CODEC_LOG_TAG "beetle_audio_codec"
#define BEETLE_AUDIO_CODEC_I2C_PORT I2C_NUM_1
#define BEETLE_AUDIO_CODEC_I2S_PORT I2S_NUM_0
#define BEETLE_AUDIO_CODEC_DMA_DESC_NUM 6
#define BEETLE_AUDIO_CODEC_DMA_FRAME_NUM 240
#define BEETLE_AUDIO_CODEC_DEFAULT_I2C_FREQ_HZ 100000U
#define BEETLE_AUDIO_CODEC_I2C_TRANS_TIMEOUT_MS 100
#define BEETLE_AUDIO_CODEC_DEFAULT_OUT_VOL 70
#define BEETLE_AUDIO_CODEC_DEFAULT_IN_GAIN 30.0f

struct beetle_audio_codec {
    i2c_master_bus_handle_t i2c_bus;
    i2s_chan_handle_t tx_handle;
    i2s_chan_handle_t rx_handle;
    const audio_codec_data_if_t *data_if;
    const audio_codec_ctrl_if_t *out_ctrl_if;
    const audio_codec_if_t *out_codec_if;
    const audio_codec_ctrl_if_t *in_ctrl_if;
    const audio_codec_if_t *in_codec_if;
    const audio_codec_gpio_if_t *gpio_if;
    esp_codec_dev_handle_t output_dev;
    esp_codec_dev_handle_t input_dev;
    int16_t *input_read_buf;
    size_t input_read_buf_samples;
    int last_esp_err;
    bool mic_enabled;
    bool speaker_enabled;
    bool input_reference;
    bool input_open;
    bool output_open;
};

typedef struct {
    audio_codec_ctrl_if_t base;
    bool is_open;
    i2c_master_bus_handle_t bus_handle;
    i2c_master_dev_handle_t dev_handle;
    uint8_t addr_8bit;
    uint32_t freq_hz;
} beetle_audio_codec_i2c_ctrl_t;

static uint8_t beetle_audio_codec_to_esp_codec_addr(uint8_t addr_7bit) {
    return (uint8_t) (addr_7bit << 1);
}

static uint32_t beetle_audio_codec_i2c_freq_or_default(uint32_t freq_hz) {
    return freq_hz == 0 ? BEETLE_AUDIO_CODEC_DEFAULT_I2C_FREQ_HZ : freq_hz;
}

static int beetle_audio_codec_i2c_ctrl_open(const audio_codec_ctrl_if_t *ctrl, void *cfg, int cfg_size) {
    (void) cfg;
    (void) cfg_size;
    if (ctrl == NULL) {
        return ESP_CODEC_DEV_INVALID_ARG;
    }
    beetle_audio_codec_i2c_ctrl_t *i2c_ctrl = (beetle_audio_codec_i2c_ctrl_t *) ctrl;
    if (i2c_ctrl->bus_handle == NULL) {
        return ESP_CODEC_DEV_INVALID_ARG;
    }
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = (uint16_t) (i2c_ctrl->addr_8bit >> 1),
        .scl_speed_hz = beetle_audio_codec_i2c_freq_or_default(i2c_ctrl->freq_hz),
    };
    int ret = i2c_master_bus_add_device(i2c_ctrl->bus_handle, &dev_cfg, &i2c_ctrl->dev_handle);
    if (ret == ESP_OK) {
        i2c_ctrl->is_open = true;
        return ESP_CODEC_DEV_OK;
    }
    return ESP_CODEC_DEV_DRV_ERR;
}

static bool beetle_audio_codec_i2c_ctrl_is_open(const audio_codec_ctrl_if_t *ctrl) {
    if (ctrl == NULL) {
        return false;
    }
    return ((const beetle_audio_codec_i2c_ctrl_t *) ctrl)->is_open;
}

static int beetle_audio_codec_i2c_ctrl_read_reg(
    const audio_codec_ctrl_if_t *ctrl,
    int addr,
    int addr_len,
    void *data,
    int data_len
) {
    uint8_t addr_data[2] = {0};
    beetle_audio_codec_i2c_ctrl_t *i2c_ctrl = (beetle_audio_codec_i2c_ctrl_t *) ctrl;
    if (ctrl == NULL || data == NULL) {
        return ESP_CODEC_DEV_INVALID_ARG;
    }
    if (!i2c_ctrl->is_open || i2c_ctrl->dev_handle == NULL) {
        return ESP_CODEC_DEV_WRONG_STATE;
    }
    if (addr_len > 1) {
        addr_data[0] = (uint8_t) (addr >> 8);
        addr_data[1] = (uint8_t) (addr & 0xff);
    } else {
        addr_data[0] = (uint8_t) (addr & 0xff);
    }
    int ret = i2c_master_transmit_receive(
        i2c_ctrl->dev_handle,
        addr_data,
        addr_len,
        data,
        data_len,
        BEETLE_AUDIO_CODEC_I2C_TRANS_TIMEOUT_MS
    );
    if (ret != ESP_OK) {
        ESP_LOGE(BEETLE_AUDIO_CODEC_LOG_TAG, "Fail to read from dev %x", i2c_ctrl->addr_8bit);
    }
    return ret ? ESP_CODEC_DEV_READ_FAIL : ESP_CODEC_DEV_OK;
}

static int beetle_audio_codec_i2c_ctrl_write_reg(
    const audio_codec_ctrl_if_t *ctrl,
    int addr,
    int addr_len,
    void *data,
    int data_len
) {
    esp_err_t ret = ESP_CODEC_DEV_NOT_SUPPORT;
    int len = addr_len + data_len;
    beetle_audio_codec_i2c_ctrl_t *i2c_ctrl = (beetle_audio_codec_i2c_ctrl_t *) ctrl;
    if (ctrl == NULL || data == NULL) {
        return ESP_CODEC_DEV_INVALID_ARG;
    }
    if (!i2c_ctrl->is_open || i2c_ctrl->dev_handle == NULL) {
        return ESP_CODEC_DEV_WRONG_STATE;
    }
    if (len <= 4) {
        uint8_t write_data[4] = {0};
        int i = 0;
        if (addr_len > 1) {
            write_data[i++] = (uint8_t) (addr >> 8);
            write_data[i++] = (uint8_t) (addr & 0xff);
        } else {
            write_data[i++] = (uint8_t) (addr & 0xff);
        }
        uint8_t *w = (uint8_t *) data;
        while (i < len) {
            write_data[i++] = *(w++);
        }
        ret = i2c_master_transmit(
            i2c_ctrl->dev_handle,
            write_data,
            len,
            BEETLE_AUDIO_CODEC_I2C_TRANS_TIMEOUT_MS
        );
    }
    if (ret != ESP_OK) {
        ESP_LOGE(BEETLE_AUDIO_CODEC_LOG_TAG, "Fail to write to dev %x", i2c_ctrl->addr_8bit);
    }
    return ret ? ESP_CODEC_DEV_WRITE_FAIL : ESP_CODEC_DEV_OK;
}

static int beetle_audio_codec_i2c_ctrl_close(const audio_codec_ctrl_if_t *ctrl) {
    if (ctrl == NULL) {
        return ESP_CODEC_DEV_INVALID_ARG;
    }
    beetle_audio_codec_i2c_ctrl_t *i2c_ctrl = (beetle_audio_codec_i2c_ctrl_t *) ctrl;
    if (i2c_ctrl->dev_handle != NULL) {
        i2c_master_bus_rm_device(i2c_ctrl->dev_handle);
        i2c_ctrl->dev_handle = NULL;
    }
    i2c_ctrl->is_open = false;
    return ESP_CODEC_DEV_OK;
}

static const audio_codec_ctrl_if_t *beetle_audio_codec_new_i2c_ctrl(
    i2c_master_bus_handle_t bus_handle,
    uint8_t addr_7bit,
    uint32_t freq_hz
) {
    beetle_audio_codec_i2c_ctrl_t *ctrl = calloc(1, sizeof(*ctrl));
    if (ctrl == NULL) {
        return NULL;
    }
    ctrl->base.open = beetle_audio_codec_i2c_ctrl_open;
    ctrl->base.is_open = beetle_audio_codec_i2c_ctrl_is_open;
    ctrl->base.read_reg = beetle_audio_codec_i2c_ctrl_read_reg;
    ctrl->base.write_reg = beetle_audio_codec_i2c_ctrl_write_reg;
    ctrl->base.close = beetle_audio_codec_i2c_ctrl_close;
    ctrl->bus_handle = bus_handle;
    ctrl->addr_8bit = beetle_audio_codec_to_esp_codec_addr(addr_7bit);
    ctrl->freq_hz = freq_hz;
    if (beetle_audio_codec_i2c_ctrl_open(&ctrl->base, NULL, 0) != ESP_CODEC_DEV_OK) {
        free(ctrl);
        return NULL;
    }
    return &ctrl->base;
}

static beetle_audio_codec_status_t beetle_audio_codec_set_error(
    beetle_audio_codec_t *codec,
    beetle_audio_codec_status_t status,
    int esp_err
) {
    if (codec != NULL) {
        codec->last_esp_err = esp_err;
    }
    return status;
}

static void beetle_audio_codec_cleanup(beetle_audio_codec_t *codec) {
    if (codec == NULL) {
        return;
    }

    if (codec->output_dev != NULL) {
        if (codec->output_open) {
            esp_codec_dev_close(codec->output_dev);
            codec->output_open = false;
        }
        esp_codec_dev_delete(codec->output_dev);
        codec->output_dev = NULL;
    }
    if (codec->input_dev != NULL) {
        if (codec->input_open) {
            esp_codec_dev_close(codec->input_dev);
            codec->input_open = false;
        }
        esp_codec_dev_delete(codec->input_dev);
        codec->input_dev = NULL;
    }

    if (codec->in_codec_if != NULL) {
        audio_codec_delete_codec_if(codec->in_codec_if);
        codec->in_codec_if = NULL;
    }
    if (codec->in_ctrl_if != NULL) {
        audio_codec_delete_ctrl_if(codec->in_ctrl_if);
        codec->in_ctrl_if = NULL;
    }
    if (codec->out_codec_if != NULL) {
        audio_codec_delete_codec_if(codec->out_codec_if);
        codec->out_codec_if = NULL;
    }
    if (codec->out_ctrl_if != NULL) {
        audio_codec_delete_ctrl_if(codec->out_ctrl_if);
        codec->out_ctrl_if = NULL;
    }
    if (codec->gpio_if != NULL) {
        audio_codec_delete_gpio_if(codec->gpio_if);
        codec->gpio_if = NULL;
    }
    if (codec->data_if != NULL) {
        audio_codec_delete_data_if(codec->data_if);
        codec->data_if = NULL;
    }

    if (codec->rx_handle != NULL) {
        i2s_channel_disable(codec->rx_handle);
        i2s_del_channel(codec->rx_handle);
        codec->rx_handle = NULL;
    }
    if (codec->tx_handle != NULL) {
        i2s_channel_disable(codec->tx_handle);
        i2s_del_channel(codec->tx_handle);
        codec->tx_handle = NULL;
    }

    if (codec->i2c_bus != NULL) {
        i2c_del_master_bus(codec->i2c_bus);
        codec->i2c_bus = NULL;
    }

    free(codec->input_read_buf);
    codec->input_read_buf = NULL;
    codec->input_read_buf_samples = 0;
}

static beetle_audio_codec_status_t beetle_audio_codec_init_i2c_bus(
    beetle_audio_codec_t *codec,
    const beetle_audio_codec_config_t *config
) {
    i2c_master_bus_config_t bus_cfg = {0};
    bus_cfg.i2c_port = BEETLE_AUDIO_CODEC_I2C_PORT;
    bus_cfg.sda_io_num = config->i2c_sda_pin;
    bus_cfg.scl_io_num = config->i2c_scl_pin;
    bus_cfg.clk_source = I2C_CLK_SRC_DEFAULT;
    bus_cfg.glitch_ignore_cnt = 7;
    bus_cfg.intr_priority = 0;
    bus_cfg.trans_queue_depth = 0;
    bus_cfg.flags.enable_internal_pullup = 1;

    int ret = i2c_new_master_bus(&bus_cfg, &codec->i2c_bus);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

static beetle_audio_codec_status_t beetle_audio_codec_init_i2s_channels(
    beetle_audio_codec_t *codec,
    const beetle_audio_codec_config_t *config,
    uint32_t sample_rate_hz
) {
    i2s_chan_config_t chan_cfg = {
        .id = BEETLE_AUDIO_CODEC_I2S_PORT,
        .role = I2S_ROLE_MASTER,
        .dma_desc_num = BEETLE_AUDIO_CODEC_DMA_DESC_NUM,
        .dma_frame_num = BEETLE_AUDIO_CODEC_DMA_FRAME_NUM,
        .auto_clear_after_cb = true,
        .auto_clear_before_cb = false,
        .intr_priority = 0,
    };

    int ret = i2s_new_channel(&chan_cfg, &codec->tx_handle, &codec->rx_handle);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }

    i2s_std_config_t tx_cfg = {
        .clk_cfg = {
            .sample_rate_hz = sample_rate_hz,
            .clk_src = I2S_CLK_SRC_DEFAULT,
            .ext_clk_freq_hz = 0,
            .mclk_multiple = I2S_MCLK_MULTIPLE_256,
        },
        .slot_cfg = {
            .data_bit_width = I2S_DATA_BIT_WIDTH_16BIT,
            .slot_bit_width = I2S_SLOT_BIT_WIDTH_AUTO,
            .slot_mode = I2S_SLOT_MODE_STEREO,
            .slot_mask = I2S_STD_SLOT_BOTH,
            .ws_width = I2S_DATA_BIT_WIDTH_16BIT,
            .ws_pol = false,
            .bit_shift = true,
            .left_align = true,
            .big_endian = false,
            .bit_order_lsb = false,
        },
        .gpio_cfg = {
            .mclk = config->i2s_mclk_pin,
            .bclk = config->i2s_bclk_pin,
            .ws = config->i2s_ws_pin,
            .dout = config->i2s_dout_pin,
            .din = I2S_GPIO_UNUSED,
            .invert_flags = {
                .mclk_inv = false,
                .bclk_inv = false,
                .ws_inv = false,
            },
        },
    };

    ret = i2s_channel_init_std_mode(codec->tx_handle, &tx_cfg);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }

    i2s_tdm_config_t rx_cfg = {
        .clk_cfg = {
            .sample_rate_hz = sample_rate_hz,
            .clk_src = I2S_CLK_SRC_DEFAULT,
            .ext_clk_freq_hz = 0,
            .mclk_multiple = I2S_MCLK_MULTIPLE_256,
            .bclk_div = 8,
        },
        .slot_cfg = {
            .data_bit_width = I2S_DATA_BIT_WIDTH_16BIT,
            .slot_bit_width = I2S_SLOT_BIT_WIDTH_AUTO,
            .slot_mode = I2S_SLOT_MODE_STEREO,
            .slot_mask = (i2s_tdm_slot_mask_t) (I2S_TDM_SLOT0 | I2S_TDM_SLOT1 |
                                                I2S_TDM_SLOT2 | I2S_TDM_SLOT3),
            .ws_width = I2S_TDM_AUTO_WS_WIDTH,
            .ws_pol = false,
            .bit_shift = true,
            .left_align = false,
            .big_endian = false,
            .bit_order_lsb = false,
            .skip_mask = false,
            .total_slot = I2S_TDM_AUTO_SLOT_NUM,
        },
        .gpio_cfg = {
            .mclk = config->i2s_mclk_pin,
            .bclk = config->i2s_bclk_pin,
            .ws = config->i2s_ws_pin,
            .dout = I2S_GPIO_UNUSED,
            .din = config->i2s_din_pin,
            .invert_flags = {
                .mclk_inv = false,
                .bclk_inv = false,
                .ws_inv = false,
            },
        },
    };

    ret = i2s_channel_init_tdm_mode(codec->rx_handle, &rx_cfg);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }

    ret = i2s_channel_enable(codec->tx_handle);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    ret = i2s_channel_enable(codec->rx_handle);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

static beetle_audio_codec_status_t beetle_audio_codec_init_data_if(
    beetle_audio_codec_t *codec
) {
    audio_codec_i2s_cfg_t i2s_cfg = {
        .port = BEETLE_AUDIO_CODEC_I2S_PORT,
        .rx_handle = codec->rx_handle,
        .tx_handle = codec->tx_handle,
    };
    codec->data_if = audio_codec_new_i2s_data(&i2s_cfg);
    if (codec->data_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

static beetle_audio_codec_status_t beetle_audio_codec_init_output_dev(
    beetle_audio_codec_t *codec,
    const beetle_audio_codec_config_t *config
) {
    if (!config->speaker_enabled) {
        return BEETLE_AUDIO_CODEC_OK;
    }

    codec->out_ctrl_if = beetle_audio_codec_new_i2c_ctrl(
        codec->i2c_bus,
        config->output_addr,
        config->i2c_freq_hz
    );
    if (codec->out_ctrl_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    codec->gpio_if = audio_codec_new_gpio();
    if (codec->gpio_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    es8311_codec_cfg_t es8311_cfg = {0};
    es8311_cfg.ctrl_if = codec->out_ctrl_if;
    es8311_cfg.gpio_if = codec->gpio_if;
    es8311_cfg.codec_mode = ESP_CODEC_DEV_WORK_MODE_DAC;
    es8311_cfg.pa_pin = config->pa_pin;
    es8311_cfg.use_mclk = true;
    es8311_cfg.hw_gain.pa_voltage = 5.0f;
    es8311_cfg.hw_gain.codec_dac_voltage = 3.3f;
    codec->out_codec_if = es8311_codec_new(&es8311_cfg);
    if (codec->out_codec_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    esp_codec_dev_cfg_t dev_cfg = {
        .dev_type = ESP_CODEC_DEV_TYPE_OUT,
        .codec_if = codec->out_codec_if,
        .data_if = codec->data_if,
    };
    codec->output_dev = esp_codec_dev_new(&dev_cfg);
    if (codec->output_dev == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    esp_codec_dev_sample_info_t fs = {
        .bits_per_sample = 16,
        .channel = 1,
        .channel_mask = 0,
        .sample_rate = (uint32_t) config->output_sample_rate_hz,
        .mclk_multiple = 0,
    };
    int ret = esp_codec_dev_open(codec->output_dev, &fs);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    codec->output_open = true;

    ret = esp_codec_dev_set_out_vol(codec->output_dev, BEETLE_AUDIO_CODEC_DEFAULT_OUT_VOL);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

static beetle_audio_codec_status_t beetle_audio_codec_init_input_dev(
    beetle_audio_codec_t *codec,
    const beetle_audio_codec_config_t *config
) {
    if (!config->mic_enabled) {
        return BEETLE_AUDIO_CODEC_OK;
    }

    codec->in_ctrl_if = beetle_audio_codec_new_i2c_ctrl(
        codec->i2c_bus,
        config->input_addr,
        config->i2c_freq_hz
    );
    if (codec->in_ctrl_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    es7210_codec_cfg_t es7210_cfg = {0};
    es7210_cfg.ctrl_if = codec->in_ctrl_if;
    es7210_cfg.mic_selected =
        ES7210_SEL_MIC1 | ES7210_SEL_MIC2 | ES7210_SEL_MIC3 | ES7210_SEL_MIC4;
    codec->in_codec_if = es7210_codec_new(&es7210_cfg);
    if (codec->in_codec_if == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    esp_codec_dev_cfg_t dev_cfg = {
        .dev_type = ESP_CODEC_DEV_TYPE_IN,
        .codec_if = codec->in_codec_if,
        .data_if = codec->data_if,
    };
    codec->input_dev = esp_codec_dev_new(&dev_cfg);
    if (codec->input_dev == NULL) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }

    esp_codec_dev_sample_info_t fs = {
        .bits_per_sample = 16,
        .channel = 4,
        .channel_mask = ESP_CODEC_DEV_MAKE_CHANNEL_MASK(0),
        .sample_rate = (uint32_t) config->input_sample_rate_hz,
        .mclk_multiple = 0,
    };
    if (config->input_reference) {
        fs.channel_mask |= ESP_CODEC_DEV_MAKE_CHANNEL_MASK(1);
    }
    int ret = esp_codec_dev_open(codec->input_dev, &fs);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    codec->input_open = true;

    ret = esp_codec_dev_set_in_channel_gain(
        codec->input_dev,
        ESP_CODEC_DEV_MAKE_CHANNEL_MASK(0),
        BEETLE_AUDIO_CODEC_DEFAULT_IN_GAIN
    );
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

beetle_audio_codec_status_t beetle_audio_codec_create(
    const beetle_audio_codec_config_t *config,
    beetle_audio_codec_t **out_codec
) {
    beetle_audio_codec_status_t status;
    beetle_audio_codec_t *codec;
    uint32_t sample_rate_hz;

    if (out_codec != NULL) {
        *out_codec = NULL;
    }
    if (config == NULL || out_codec == NULL) {
        return BEETLE_AUDIO_CODEC_ERR_INVALID_ARG;
    }
    if (!config->mic_enabled && !config->speaker_enabled) {
        return BEETLE_AUDIO_CODEC_ERR_INVALID_ARG;
    }
    if (config->mic_enabled && config->speaker_enabled &&
        config->input_sample_rate_hz != config->output_sample_rate_hz) {
        return BEETLE_AUDIO_CODEC_ERR_INVALID_ARG;
    }

    codec = calloc(1, sizeof(*codec));
    if (codec == NULL) {
        return BEETLE_AUDIO_CODEC_ERR_NOMEM;
    }

    codec->mic_enabled = config->mic_enabled;
    codec->speaker_enabled = config->speaker_enabled;
    codec->input_reference = config->input_reference;
    sample_rate_hz = config->mic_enabled
        ? (uint32_t) config->input_sample_rate_hz
        : (uint32_t) config->output_sample_rate_hz;

    status = beetle_audio_codec_init_i2c_bus(codec, config);
    if (status != BEETLE_AUDIO_CODEC_OK) {
        goto fail;
    }
    status = beetle_audio_codec_init_i2s_channels(codec, config, sample_rate_hz);
    if (status != BEETLE_AUDIO_CODEC_OK) {
        goto fail;
    }
    status = beetle_audio_codec_init_data_if(codec);
    if (status != BEETLE_AUDIO_CODEC_OK) {
        goto fail;
    }
    status = beetle_audio_codec_init_output_dev(codec, config);
    if (status != BEETLE_AUDIO_CODEC_OK) {
        goto fail;
    }
    status = beetle_audio_codec_init_input_dev(codec, config);
    if (status != BEETLE_AUDIO_CODEC_OK) {
        goto fail;
    }

    ESP_LOGI(
        BEETLE_AUDIO_CODEC_LOG_TAG,
        "codec ready sr_in=%d sr_out=%d mic=%d speaker=%d ref=%d addrs=0x%02X/0x%02X",
        config->input_sample_rate_hz,
        config->output_sample_rate_hz,
        config->mic_enabled,
        config->speaker_enabled,
        config->input_reference,
        config->input_addr,
        config->output_addr
    );
    *out_codec = codec;
    return BEETLE_AUDIO_CODEC_OK;

fail:
    beetle_audio_codec_cleanup(codec);
    free(codec);
    return status;
}

void beetle_audio_codec_destroy(beetle_audio_codec_t *codec) {
    if (codec == NULL) {
        return;
    }
    beetle_audio_codec_cleanup(codec);
    free(codec);
}

beetle_audio_codec_status_t beetle_audio_codec_read_mic_pcm16(
    beetle_audio_codec_t *codec,
    int16_t *out_samples,
    size_t sample_count,
    size_t *out_samples_read
) {
    size_t bytes_to_read;
    int ret;

    if (codec == NULL || out_samples == NULL || out_samples_read == NULL) {
        return BEETLE_AUDIO_CODEC_ERR_INVALID_ARG;
    }
    if (!codec->mic_enabled || codec->input_dev == NULL || !codec->input_open) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }
    if (sample_count == 0) {
        *out_samples_read = 0;
        return BEETLE_AUDIO_CODEC_OK;
    }

    if (!codec->input_reference) {
        bytes_to_read = sample_count * sizeof(int16_t);
        ret = esp_codec_dev_read(codec->input_dev, out_samples, bytes_to_read);
        if (ret != ESP_OK) {
            return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
        }
        *out_samples_read = sample_count;
        return BEETLE_AUDIO_CODEC_OK;
    }

    if (codec->input_read_buf_samples < sample_count * 2U) {
        int16_t *new_buf = realloc(codec->input_read_buf, sample_count * 2U * sizeof(int16_t));
        if (new_buf == NULL) {
            return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_NOMEM, 0);
        }
        codec->input_read_buf = new_buf;
        codec->input_read_buf_samples = sample_count * 2U;
    }

    bytes_to_read = sample_count * 2U * sizeof(int16_t);
    ret = esp_codec_dev_read(codec->input_dev, codec->input_read_buf, bytes_to_read);
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    for (size_t i = 0; i < sample_count; ++i) {
        out_samples[i] = codec->input_read_buf[i * 2U];
    }
    *out_samples_read = sample_count;
    return BEETLE_AUDIO_CODEC_OK;
}

beetle_audio_codec_status_t beetle_audio_codec_write_speaker_pcm16(
    beetle_audio_codec_t *codec,
    const int16_t *samples,
    size_t sample_count
) {
    int ret;

    if (codec == NULL || samples == NULL) {
        return BEETLE_AUDIO_CODEC_ERR_INVALID_ARG;
    }
    if (!codec->speaker_enabled || codec->output_dev == NULL || !codec->output_open) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_STATE, 0);
    }
    if (sample_count == 0) {
        return BEETLE_AUDIO_CODEC_OK;
    }

    ret = esp_codec_dev_write(codec->output_dev, (void *) samples, sample_count * sizeof(int16_t));
    if (ret != ESP_OK) {
        return beetle_audio_codec_set_error(codec, BEETLE_AUDIO_CODEC_ERR_ESP, ret);
    }
    return BEETLE_AUDIO_CODEC_OK;
}

int beetle_audio_codec_last_esp_err(const beetle_audio_codec_t *codec) {
    if (codec == NULL) {
        return 0;
    }
    return codec->last_esp_err;
}
