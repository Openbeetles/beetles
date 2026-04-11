/// 简易精简：去掉块注释与行注释，多空行压成一行，减少 Flash 占用。
/// 按 UTF-8 码点推进，避免把中文等多字节字符拆成单字节导致乱码。
fn minify_content(content: &str, strip_line_comment: bool, strip_block_comment: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if strip_line_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if strip_block_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
            continue;
        }
        if i + 3 < bytes.len()
            && bytes[i] == b'<'
            && bytes[i + 1] == b'!'
            && bytes[i + 2] == b'-'
            && bytes[i + 3] == b'-'
        {
            i += 4;
            while i + 2 < bytes.len()
                && !(bytes[i] == b'-' && bytes[i + 1] == b'-' && bytes[i + 2] == b'>')
            {
                i += 1;
            }
            if i + 2 < bytes.len() {
                i += 3;
            }
            continue;
        }
        let b = bytes[i];
        if b == b'\n' || b == b'\r' {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            i += 1;
            continue;
        }
        // 按 UTF-8 码点推进，避免多字节字符被拆成单字节 (b as char) 导致乱码
        if let Ok(rest) = std::str::from_utf8(&bytes[i..]) {
            if let Some(ch) = rest.chars().next() {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
        }
        i += 1;
    }
    out.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

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
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    maybe_overlay_esp_sr_p4_eco5_libs(&target, &out_dir);

    let min_dir = out_dir.join("config_page_min");
    let _ = std::fs::create_dir_all(&min_dir);
    let src_dir = manifest.join("src/platform/config_page");
    // common.js 内翻译字符串含 "http://" 等，若剥离行注释会误删导致语法错误，故不剥离行注释；
    // 块注释同理，字符串内可能含 /* 序列，也不剥离，保证功能正确优先。
    let files = [
        ("common.js", false, false),
        ("common.css", false, true),
        ("wifi_config_page.html", false, false),
        ("pairing_page.html", false, false),
    ];
    for (name, line_comment, block_comment) in files {
        let src = src_dir.join(name);
        if let Ok(s) = std::fs::read_to_string(&src) {
            let minified = minify_content(&s, line_comment, block_comment);
            let _ = std::fs::write(min_dir.join(name), &minified);
        }
    }
    println!("cargo:rerun-if-changed=src/platform/config_page/");

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
