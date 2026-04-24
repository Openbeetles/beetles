//! Firmware identity lines printed at startup.
//! ESP-specific flash and app descriptor reads stay behind the platform boundary.

use crate::build_info;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use esp_idf_svc::sys;

const TAG: &str = "firmware_identity";

/// Build and firmware identity lines suitable for early startup logs.
pub fn startup_identity_lines() -> Vec<String> {
    let mut lines = vec![format!(
        "[{}] build git_sha={} dirty={} build_time_utc={} partition_csv_sha256={}",
        TAG,
        build_info::build_git_sha(),
        build_info::build_git_dirty(),
        build_info::build_time_utc(),
        build_info::partition_csv_sha256(),
    )];
    lines.extend(target_identity_lines());
    lines
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn target_identity_lines() -> Vec<String> {
    Vec::new()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn target_identity_lines() -> Vec<String> {
    let mut lines = Vec::with_capacity(3);
    lines.push(esp_app_identity_line());
    lines.push(esp_partition_table_identity_line());
    if let Some(line) = esp_partition_layout_line() {
        lines.push(line);
    }
    lines
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn esp_app_identity_line() -> String {
    let desc = unsafe { sys::esp_app_get_description() };
    if desc.is_null() {
        return format!("[{}] esp_app unavailable", TAG);
    }

    let desc = unsafe { &*desc };
    format!(
        "[{}] esp_app project={} version={} compile={} {} idf={} elf_sha256={}",
        TAG,
        c_char_buf_to_string(&desc.project_name),
        c_char_buf_to_string(&desc.version),
        c_char_buf_to_string(&desc.date),
        c_char_buf_to_string(&desc.time),
        c_char_buf_to_string(&desc.idf_ver),
        hex_bytes(&desc.app_elf_sha256),
    )
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn esp_partition_table_identity_line() -> String {
    const PARTITION_TABLE_OFFSET: u32 = 0x8000;
    const PARTITION_TABLE_IMAGE_LEN: usize = 0x0c00;

    let mut image = [0_u8; PARTITION_TABLE_IMAGE_LEN];
    let status = unsafe {
        sys::esp_flash_read(
            core::ptr::null_mut(),
            image.as_mut_ptr().cast(),
            PARTITION_TABLE_OFFSET,
            PARTITION_TABLE_IMAGE_LEN as u32,
        )
    };
    if status != sys::ESP_OK {
        return format!(
            "[{}] partition_table offset=0x{:x} len={} sha256=unavailable esp_err={}",
            TAG, PARTITION_TABLE_OFFSET, PARTITION_TABLE_IMAGE_LEN, status
        );
    }

    match sha256_hex(&image) {
        Some(sha) => format!(
            "[{}] partition_table offset=0x{:x} len={} sha256={}",
            TAG, PARTITION_TABLE_OFFSET, PARTITION_TABLE_IMAGE_LEN, sha
        ),
        None => format!(
            "[{}] partition_table offset=0x{:x} len={} sha256=unavailable",
            TAG, PARTITION_TABLE_OFFSET, PARTITION_TABLE_IMAGE_LEN
        ),
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn esp_partition_layout_line() -> Option<String> {
    let mut entries: Vec<(u32, String)> = Vec::new();
    let mut iterator = unsafe {
        sys::esp_partition_find(
            sys::esp_partition_type_t_ESP_PARTITION_TYPE_ANY,
            sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_ANY,
            core::ptr::null(),
        )
    };

    while !iterator.is_null() {
        let partition = unsafe { sys::esp_partition_get(iterator) };
        if !partition.is_null() {
            let partition = unsafe { &*partition };
            let label = c_char_buf_to_string(&partition.label);
            entries.push((
                partition.address,
                format!(
                    "{}:type=0x{:02x}:sub=0x{:02x}:off=0x{:x}:size=0x{:x}:enc={}:ro={}",
                    label,
                    partition.type_,
                    partition.subtype,
                    partition.address,
                    partition.size,
                    partition.encrypted as u8,
                    partition.readonly as u8,
                ),
            ));
        }
        iterator = unsafe { sys::esp_partition_next(iterator) };
    }
    unsafe { sys::esp_partition_iterator_release(iterator) };

    if entries.is_empty() {
        return None;
    }

    entries.sort_by_key(|(address, _)| *address);
    let has_model_partition = entries.iter().any(|(_, entry)| entry.starts_with("model:"));
    let joined = entries
        .iter()
        .map(|(_, entry)| entry.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let layout_sha = sha256_hex(joined.as_bytes()).unwrap_or_else(|| "unavailable".to_string());
    let mismatch_hint = if has_model_partition {
        " partition_layout_mismatch=true action=full_flash_current_layout_required"
    } else {
        " partition_layout_mismatch=false"
    };
    Some(format!(
        "[{}] partition_layout count={} sha256={}{} entries={}",
        TAG,
        entries.len(),
        layout_sha,
        mismatch_hint,
        joined
    ))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn c_char_buf_to_string(buf: &[core::ffi::c_char]) -> String {
    unsafe { core::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn sha256_hex(bytes: &[u8]) -> Option<String> {
    let mut digest = [0_u8; 32];
    let status =
        unsafe { sys::mbedtls_sha256(bytes.as_ptr(), bytes.len(), digest.as_mut_ptr(), 0) };
    (status == 0).then(|| hex_bytes(&digest))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
