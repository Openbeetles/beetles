if(IDF_TARGET STREQUAL "esp32s3")
    idf_component_register(
        REQUIRES espressif__cjson
    )
else()
    idf_component_register()
endif()
