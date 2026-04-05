//! Linux storage media discovery via `/proc/self/mountinfo` + `/sys/class/block`.
//! 基于 `/proc/self/mountinfo` 与 `/sys/class/block` 的 Linux 存储介质探测。

use crate::error::Result;
use crate::platform::StorageMediaInfo;

#[cfg(target_os = "linux")]
mod imp {
    use crate::error::{Error, Result};
    use crate::platform::{StorageMediaInfo, StorageMediaKind};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    const STAGE: &str = "storage_media";
    const SECTOR_BYTES: u64 = 512;

    #[derive(Clone, Debug)]
    struct MountInfo {
        mount_point: PathBuf,
        fs_type: String,
        source: String,
    }

    #[derive(Clone, Debug)]
    struct DeviceMountSummary {
        mounted: bool,
        mount_path: Option<String>,
        filesystem: Option<String>,
        source: Option<String>,
        is_system_root: bool,
        is_state_root: bool,
    }

    #[derive(Clone, Debug)]
    struct BlockDeviceProbe {
        base_name: String,
        label: String,
        kind: StorageMediaKind,
        removable: bool,
        capacity_bytes: Option<u64>,
    }

    pub fn discover() -> Result<Vec<StorageMediaInfo>> {
        let state_root = crate::platform::state_root::state_mount_path();
        let mounts = read_mounts()?;
        let root_mount = find_deepest_mount(Path::new("/"), &mounts);
        let state_mount = find_deepest_mount(state_root.as_path(), &mounts);

        let mut summaries: BTreeMap<String, DeviceMountSummary> = BTreeMap::new();
        let mut synthetic = Vec::new();

        if let Some(root_mount) = root_mount.clone() {
            record_mount(
                &mut summaries,
                &mut synthetic,
                &root_mount,
                true,
                state_mount
                    .as_ref()
                    .is_some_and(|state| state.mount_point == root_mount.mount_point),
            );
        }
        if let Some(state_mount) = state_mount.clone() {
            let is_root = root_mount
                .as_ref()
                .is_some_and(|root| root.mount_point == state_mount.mount_point);
            if !is_root {
                record_mount(&mut summaries, &mut synthetic, &state_mount, false, true);
            }
        }

        let mut media = enumerate_block_devices(&summaries)?;
        media.extend(synthetic);
        media.sort_by(|a, b| {
            b.is_state_root
                .cmp(&a.is_state_root)
                .then_with(|| b.is_system_root.cmp(&a.is_system_root))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(media)
    }

    fn read_mounts() -> Result<Vec<MountInfo>> {
        let content =
            fs::read_to_string("/proc/self/mountinfo").map_err(|e| Error::io(STAGE, e))?;
        let mut mounts = Vec::new();
        for line in content.lines() {
            let Some((left, right)) = line.split_once(" - ") else {
                continue;
            };
            let left_fields: Vec<&str> = left.split_whitespace().collect();
            let right_fields: Vec<&str> = right.split_whitespace().collect();
            if left_fields.len() < 5 || right_fields.len() < 2 {
                continue;
            }
            mounts.push(MountInfo {
                mount_point: PathBuf::from(unescape_mount_field(left_fields[4])),
                fs_type: right_fields[0].to_string(),
                source: unescape_mount_field(right_fields[1]),
            });
        }
        Ok(mounts)
    }

    fn find_deepest_mount(path: &Path, mounts: &[MountInfo]) -> Option<MountInfo> {
        mounts
            .iter()
            .filter(|mount| path.starts_with(mount.mount_point.as_path()))
            .max_by_key(|mount| mount.mount_point.components().count())
            .cloned()
    }

    fn record_mount(
        summaries: &mut BTreeMap<String, DeviceMountSummary>,
        synthetic: &mut Vec<StorageMediaInfo>,
        mount: &MountInfo,
        is_system_root: bool,
        is_state_root: bool,
    ) {
        let usage = statvfs_usage(&mount.mount_point);
        if let Some(base_name) = block_base_name_from_source(mount.source.as_str()) {
            let entry = summaries
                .entry(base_name)
                .or_insert_with(|| DeviceMountSummary {
                    mounted: true,
                    mount_path: None,
                    filesystem: None,
                    source: None,
                    is_system_root: false,
                    is_state_root: false,
                });
            entry.mounted = true;
            entry.is_system_root |= is_system_root;
            entry.is_state_root |= is_state_root;
            prefer_mount(entry, mount, is_system_root, is_state_root);
            return;
        }

        synthetic.push(StorageMediaInfo {
            id: format!("mount:{}", mount.mount_point.display()),
            kind: classify_virtual_kind(mount.fs_type.as_str()),
            label: format!("{} ({})", mount.source, mount.mount_point.display()),
            present: true,
            mounted: true,
            mount_path: Some(mount.mount_point.display().to_string()),
            filesystem: Some(mount.fs_type.clone()),
            source: Some(mount.source.clone()),
            removable: false,
            is_system_root,
            is_state_root,
            capacity_bytes: usage.map(|(total, _)| total),
            free_bytes: usage.map(|(_, free)| free),
        });
    }

    fn prefer_mount(
        summary: &mut DeviceMountSummary,
        mount: &MountInfo,
        is_system_root: bool,
        is_state_root: bool,
    ) {
        let candidate = mount.mount_point.display().to_string();
        let replace = match summary.mount_path.as_deref() {
            None => true,
            Some(current) => {
                let current_score = mount_priority(
                    current,
                    summary.is_system_root && current == "/",
                    summary.is_state_root,
                );
                let candidate_score =
                    mount_priority(candidate.as_str(), is_system_root, is_state_root);
                candidate_score > current_score
            }
        };
        if replace {
            summary.mount_path = Some(candidate);
            summary.filesystem = Some(mount.fs_type.clone());
            summary.source = Some(mount.source.clone());
        }
    }

    fn mount_priority(path: &str, is_system_root: bool, is_state_root: bool) -> u8 {
        if is_state_root && path != "/" {
            3
        } else if is_state_root {
            2
        } else if is_system_root {
            1
        } else {
            0
        }
    }

    fn enumerate_block_devices(
        summaries: &BTreeMap<String, DeviceMountSummary>,
    ) -> Result<Vec<StorageMediaInfo>> {
        let mut media = Vec::new();
        let mut seen = BTreeSet::new();
        let entries = fs::read_dir("/sys/class/block").map_err(|e| Error::io(STAGE, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(STAGE, e))?;
            let base_name = entry.file_name().to_string_lossy().to_string();
            if is_partition(entry.path().as_path()) {
                continue;
            }
            let Some(probe) = probe_block_device(base_name.as_str()) else {
                continue;
            };
            let summary = summaries.get(base_name.as_str());
            seen.insert(base_name.clone());
            media.push(StorageMediaInfo {
                id: probe.base_name.clone(),
                kind: probe.kind,
                label: probe.label,
                present: true,
                mounted: summary.is_some_and(|s| s.mounted),
                mount_path: summary.and_then(|s| s.mount_path.clone()),
                filesystem: summary.and_then(|s| s.filesystem.clone()),
                source: summary.and_then(|s| s.source.clone()),
                removable: probe.removable,
                is_system_root: summary.is_some_and(|s| s.is_system_root),
                is_state_root: summary.is_some_and(|s| s.is_state_root),
                capacity_bytes: probe.capacity_bytes,
                free_bytes: summary
                    .and_then(|s| s.mount_path.as_deref())
                    .and_then(|path| statvfs_usage(Path::new(path)).map(|(_, free)| free)),
            });
        }

        for (base_name, summary) in summaries {
            if seen.contains(base_name) {
                continue;
            }
            media.push(StorageMediaInfo {
                id: base_name.clone(),
                kind: StorageMediaKind::Unknown,
                label: summary.source.clone().unwrap_or_else(|| base_name.clone()),
                present: summary.source.is_some(),
                mounted: summary.mounted,
                mount_path: summary.mount_path.clone(),
                filesystem: summary.filesystem.clone(),
                source: summary.source.clone(),
                removable: false,
                is_system_root: summary.is_system_root,
                is_state_root: summary.is_state_root,
                capacity_bytes: summary
                    .mount_path
                    .as_deref()
                    .and_then(|path| statvfs_usage(Path::new(path)).map(|(total, _)| total)),
                free_bytes: summary
                    .mount_path
                    .as_deref()
                    .and_then(|path| statvfs_usage(Path::new(path)).map(|(_, free)| free)),
            });
        }

        Ok(media)
    }

    fn probe_block_device(base_name: &str) -> Option<BlockDeviceProbe> {
        let sys_path = Path::new("/sys/class/block").join(base_name);
        let removable =
            read_trimmed(sys_path.join("removable").as_path()).is_some_and(|v| v == "1");
        let dev_type = read_trimmed(sys_path.join("device/type").as_path());
        let model = read_trimmed(sys_path.join("device/model").as_path())
            .or_else(|| read_trimmed(sys_path.join("device/name").as_path()));
        let size_sectors =
            read_trimmed(sys_path.join("size").as_path()).and_then(|v| v.parse::<u64>().ok());
        let capacity_bytes = size_sectors.map(|sectors| sectors.saturating_mul(SECTOR_BYTES));
        let kind = classify_block_device(
            base_name,
            dev_type.as_deref(),
            removable,
            sys_path.as_path(),
        );
        if matches!(kind, StorageMediaKind::Unknown) {
            return None;
        }
        let label = model
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| base_name.to_string());
        Some(BlockDeviceProbe {
            base_name: base_name.to_string(),
            label,
            kind,
            removable,
            capacity_bytes,
        })
    }

    fn classify_block_device(
        base_name: &str,
        dev_type: Option<&str>,
        removable: bool,
        sys_path: &Path,
    ) -> StorageMediaKind {
        if base_name.starts_with("mmcblk") {
            return match dev_type.unwrap_or_default() {
                "SD" | "SDHC" | "SDXC" => StorageMediaKind::SdCard,
                "MMC" => StorageMediaKind::Emmc,
                _ if removable => StorageMediaKind::SdCard,
                _ => StorageMediaKind::Emmc,
            };
        }
        if base_name.starts_with("nvme") {
            return StorageMediaKind::Nvme;
        }
        if base_name.starts_with("sd")
            || base_name.starts_with("hd")
            || base_name.starts_with("vd")
            || base_name.starts_with("xvd")
        {
            if removable || device_path_has_usb(sys_path) {
                return StorageMediaKind::UsbMassStorage;
            }
        }
        StorageMediaKind::Unknown
    }

    fn device_path_has_usb(sys_path: &Path) -> bool {
        fs::canonicalize(sys_path.join("device"))
            .ok()
            .is_some_and(|path| path.to_string_lossy().contains("/usb"))
    }

    fn classify_virtual_kind(fs_type: &str) -> StorageMediaKind {
        match fs_type {
            "overlay" | "tmpfs" | "squashfs" | "ramfs" => StorageMediaKind::Virtual,
            _ => StorageMediaKind::Unknown,
        }
    }

    fn statvfs_usage(path: &Path) -> Option<(u64, u64)> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut vfs) } != 0 {
            return None;
        }
        let frsize = vfs.f_frsize as u64;
        let total = (vfs.f_blocks as u64).saturating_mul(frsize);
        let free = (vfs.f_bavail as u64).saturating_mul(frsize);
        Some((total, free))
    }

    fn is_partition(path: &Path) -> bool {
        path.join("partition").exists()
    }

    fn read_trimmed(path: &Path) -> Option<String> {
        fs::read_to_string(path)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn block_base_name_from_source(source: &str) -> Option<String> {
        let dev_path = normalized_device_path(source)?;
        let file_name = dev_path.file_name()?.to_string_lossy().to_string();
        Some(base_block_name(file_name.as_str()))
    }

    fn normalized_device_path(source: &str) -> Option<PathBuf> {
        if !source.starts_with("/dev/") {
            return None;
        }
        let path = PathBuf::from(source);
        fs::canonicalize(&path).ok().or(Some(path))
    }

    fn base_block_name(device_name: &str) -> String {
        if let Some(prefix) = trim_partition_suffix(device_name, "mmcblk", 'p') {
            return prefix;
        }
        if let Some(prefix) = trim_partition_suffix(device_name, "nvme", 'p') {
            return prefix;
        }
        let trimmed = device_name.trim_end_matches(|c: char| c.is_ascii_digit());
        if trimmed != device_name {
            return trimmed.to_string();
        }
        device_name.to_string()
    }

    fn trim_partition_suffix(device_name: &str, prefix: &str, marker: char) -> Option<String> {
        if !device_name.starts_with(prefix) {
            return None;
        }
        let marker_pos = device_name.rfind(marker)?;
        let suffix = &device_name[marker_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(device_name[..marker_pos].to_string());
        }
        None
    }

    fn unescape_mount_field(value: &str) -> String {
        let bytes = value.as_bytes();
        let mut out = String::with_capacity(value.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 3 < bytes.len() {
                let octal = &value[i + 1..i + 4];
                if octal.chars().all(|ch| ('0'..='7').contains(&ch)) {
                    if let Ok(parsed) = u8::from_str_radix(octal, 8) {
                        out.push(parsed as char);
                        i += 4;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::{base_block_name, unescape_mount_field};

        #[test]
        fn strips_mmc_partition_suffix() {
            assert_eq!(base_block_name("mmcblk0p2"), "mmcblk0");
            assert_eq!(base_block_name("mmcblk1"), "mmcblk1");
        }

        #[test]
        fn strips_nvme_partition_suffix() {
            assert_eq!(base_block_name("nvme0n1p3"), "nvme0n1");
        }

        #[test]
        fn unescapes_mountinfo_fields() {
            assert_eq!(unescape_mount_field("/media/My\\040Card"), "/media/My Card");
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::error::Result;
    use crate::platform::StorageMediaInfo;

    pub fn discover() -> Result<Vec<StorageMediaInfo>> {
        Ok(Vec::new())
    }
}

pub(crate) fn discover() -> Result<Vec<StorageMediaInfo>> {
    imp::discover()
}
