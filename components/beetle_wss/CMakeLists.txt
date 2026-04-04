idf_component_register(
    SRCS "beetle_wss.c"
    INCLUDE_DIRS "include"
    REQUIRES esp-tls esp_timer esp_hw_support lwip
)
