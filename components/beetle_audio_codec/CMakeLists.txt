idf_component_register(
    SRCS "beetle_audio_codec.c"
    INCLUDE_DIRS "include"
    REQUIRES
        esp_driver_gpio
        esp_driver_i2c
        esp_driver_i2s
        esp_codec_dev
        esp_audio_codec
)
