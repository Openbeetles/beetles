if((IDF_TARGET STREQUAL "esp32s3") OR (IDF_TARGET STREQUAL "esp32p4"))
    idf_component_register(
        REQUIRES espressif__cjson
    )
else()
    idf_component_register()
endif()
