# Beetle Vendor Notes

This directory vendors the upstream `esp-hosted-mcu` source used for the board-mounted
`ESP32-C6` hosted slave firmware on `ESP32-P4-NANO`.

- Upstream project: `https://github.com/espressif/esp-hosted-mcu`
- Upstream version: `2.12.3`
- Pinned upstream commit: `cbc1110dd34c51f6372430ab49251cc2ce5cc1b9`
- Vendored subset: `common/`, `docs/`, `slave/`, root `CMakeLists.txt`, `Kconfig`,
  `idf_component.yml`, `.gitignore`, `LICENSE`, `README.upstream.md`

Why this exists:

- Beetle runs only on `ESP32-P4`
- The on-board `ESP32-C6` does not run Beetle
- But the board is not product-complete unless the official hosted slave firmware is
  version-pinned, buildable, and flashable from this repository

Beetle-specific integration lives outside the upstream codepath as much as possible:

- host-side P4 settings stay in Beetle `sdkconfig.defaults.esp32p4*`
- C6 board selection overlay lives in
  `sdkconfig.defaults.esp32c6.hosted_p4_function_board`
- operational entrypoints are `build.sh build-c6`, `build.sh flash-c6`,
  `build.sh flash-all` and their PowerShell equivalents

Build reproducibility note:

- Beetle vendors `slave/components/cmd_system` so the hosted slave manifest no longer
  depends on `${IDF_PATH}` for that local component
- `slave/dependencies.lock` is committed and expected to stay machine-independent
- the remaining service dependencies are pinned by that lock file after the first verified
  Beetle-owned build
