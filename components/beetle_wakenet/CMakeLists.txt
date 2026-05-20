if(IDF_TARGET STREQUAL "esp32s3")
    idf_component_register(
        SRCS "beetle_wakenet.c"
        INCLUDE_DIRS "include"
        PRIV_REQUIRES esp-sr heap
    )

    if(IDF_VERSION_MAJOR GREATER_EQUAL 6)
        idf_component_get_property(esp_dsp_lib espressif__esp-dsp COMPONENT_LIB)
        if(esp_dsp_lib)
            target_compile_options(${esp_dsp_lib} PRIVATE
                $<$<COMPILE_LANGUAGE:CXX>:-include>
                $<$<COMPILE_LANGUAGE:CXX>:cmath>
            )
        endif()
    endif()
else()
    idf_component_register()
endif()
