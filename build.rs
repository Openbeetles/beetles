fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=BEETLE_PACKAGE_PROFILE");
    println!("cargo:rerun-if-env-changed=BEETLE_BUILD_GIT_SHA");
    println!("cargo:rerun-if-env-changed=BEETLE_BUILD_GIT_DIRTY");
    println!("cargo:rerun-if-env-changed=BEETLE_BUILD_TIME_UTC");
    println!("cargo:rerun-if-env-changed=BEETLE_PARTITION_CSV_SHA256");
    println!("cargo:rerun-if-env-changed=ESP_IDF_SDKCONFIG_DEFAULTS");
    if let Ok(profile) = std::env::var("BEETLE_PACKAGE_PROFILE") {
        let trimmed = profile.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=BEETLE_PACKAGE_PROFILE={}", trimmed);
        }
    }
    for key in [
        "BEETLE_BUILD_GIT_SHA",
        "BEETLE_BUILD_GIT_DIRTY",
        "BEETLE_BUILD_TIME_UTC",
        "BEETLE_PARTITION_CSV_SHA256",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                println!("cargo:rustc-env={}={}", key, trimmed);
            }
        }
    }
    // Artifact target triple: Xtensa or ESP-IDF RISC-V only (avoid matching unrelated "esp" substrings).
    let is_esp =
        target.contains("xtensa") || (target.contains("riscv32") && target.contains("espidf"));
    if is_esp {
        embuild::espidf::sysenv::output();
    }
    // 声明自定义 cfg，避免 unexpected_cfgs 警告（http_client 等处使用）。
    println!("cargo:rustc-check-cfg=cfg(esp_idf_version_major, values(\"4\"))");
    println!("cargo:rustc-check-cfg=cfg(beetle_esp_sr_wakenet)");

    // 为看门狗 API 选择提供 esp_idf_version_major：IDF 4.x 用 esp_task_wdt_feed，5.x 用 esp_task_wdt_reset。
    // 从 IDF_PATH/version.txt 解析；未设置 IDF_PATH 时默认 5（常见于 espup 等）。
    if is_esp {
        let idf_path = std::env::var("IDF_PATH").ok();
        let version_path = idf_path
            .as_ref()
            .map(|p| std::path::Path::new(p).join("version.txt"));
        let version_txt = version_path.and_then(|p| std::fs::read_to_string(p).ok());
        let major = version_txt
            .as_ref()
            .and_then(|s| {
                s.trim()
                    .split('.')
                    .next()
                    .and_then(|m| m.parse::<u32>().ok())
            })
            .unwrap_or(5);
        println!("cargo:rustc-cfg=esp_idf_version_major=\"{}\"", major);
        let idf_version = version_txt
            .as_ref()
            .and_then(|s| s.lines().next())
            .map(|l| l.trim().to_string())
            .or_else(|| std::env::var("ESP_IDF_VERSION").ok())
            .unwrap_or_else(|| "unknown".to_string());
        println!("cargo:rustc-env=IDF_VERSION={}", idf_version);

        let sdkconfig_defaults = std::env::var("ESP_IDF_SDKCONFIG_DEFAULTS").unwrap_or_default();
        if target.contains("esp32s3")
            || sdkconfig_defaults.contains("esp32s3")
            || sdkconfig_defaults.contains("esp32p4")
        {
            println!("cargo:rustc-cfg=beetle_esp_sr_wakenet");
        }
    }
}
