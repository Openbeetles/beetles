fn maybe_overlay_esp_sr_p4_eco5_libs(target: &str, out_dir: &std::path::Path) {
    if target != "riscv32imafc-esp-espidf" {
        return;
    }

    let Some(build_root) = out_dir.ancestors().nth(2) else {
        return;
    };

    let Ok(entries) = std::fs::read_dir(build_root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("esp-idf-sys-") {
            continue;
        }

        let managed_root = path.join("out/managed_components/espressif__esp-sr/lib");
        let legacy_dir = managed_root.join("esp32p4");
        let eco5_dir = managed_root.join("esp32p4_eco5");
        if !legacy_dir.is_dir() || !eco5_dir.is_dir() {
            continue;
        }

        let Ok(eco5_entries) = std::fs::read_dir(&eco5_dir) else {
            continue;
        };

        for eco5_entry in eco5_entries.flatten() {
            let eco5_path = eco5_entry.path();
            let is_archive = eco5_path.extension().and_then(|ext| ext.to_str()) == Some("a");
            if !is_archive {
                continue;
            }
            let Some(file_name) = eco5_path.file_name() else {
                continue;
            };
            let legacy_path = legacy_dir.join(file_name);
            if legacy_path.exists() {
                std::fs::copy(&eco5_path, &legacy_path).unwrap_or_else(|error| {
                    panic!(
                        "failed to overlay ESP-SR ESP32-P4 eco5 library {} -> {}: {}",
                        eco5_path.display(),
                        legacy_path.display(),
                        error
                    )
                });
            }
        }
    }
}

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=BEETLE_PACKAGE_PROFILE");
    println!("cargo:rerun-if-env-changed=ESP_IDF_SDKCONFIG_DEFAULTS");
    if let Ok(profile) = std::env::var("BEETLE_PACKAGE_PROFILE") {
        let trimmed = profile.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=BEETLE_PACKAGE_PROFILE={}", trimmed);
        }
    }
    // Artifact target triple: Xtensa or ESP-IDF RISC-V only (avoid matching unrelated "esp" substrings).
    let is_esp =
        target.contains("xtensa") || (target.contains("riscv32") && target.contains("espidf"));
    if is_esp {
        embuild::espidf::sysenv::output();
    }

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    maybe_overlay_esp_sr_p4_eco5_libs(&target, &out_dir);

    // 声明自定义 cfg，避免 unexpected_cfgs 警告（http_client 等处使用）。
    println!("cargo:rustc-check-cfg=cfg(esp_idf_version_major, values(\"4\"))");

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
    }
}
