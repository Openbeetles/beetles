# ESP-IDF v6.0 不再提供名为 `json` 的内置组件（改为 Registry 的 espressif/cjson）。
# esp-sr 等第三方仍 REQUIRES json；本目录名即为组件名 `json`，链到 espressif__cjson。
# ESP-IDF v6.0 dropped the in-tree `json` component (use espressif/cjson from the registry).
# Third-party components like esp-sr still REQUIRES json; this directory registers as `json`.
idf_component_register(
    SRCS "json_compat.c"
    REQUIRES espressif__cjson
)
