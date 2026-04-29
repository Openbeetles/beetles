//! Mode-aware PSRAM cache pool policy.
//! 按运行模式/压力持有可驱逐 PSRAM cache，避免把“提高 PSRAM 使用率”变成常驻内存占用。

use crate::orchestrator::PressureLevel;
use crate::platform::ByteBuffer;

/// PSRAM pool role. Roles stay explicit so future display/audio/camera pools do not share budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsramPoolRole {
    DisplayAssetCache,
    AudioInput,
    AudioOutput,
    CameraFrame,
}

impl PsramPoolRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DisplayAssetCache => "display_asset_cache",
            Self::AudioInput => "audio_input",
            Self::AudioOutput => "audio_output",
            Self::CameraFrame => "camera_frame",
        }
    }
}

/// Admission fact for one mode-owned PSRAM pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsramPoolAdmission {
    pub role: PsramPoolRole,
    pub enabled: bool,
    pub pressure: PressureLevel,
    pub budget_bytes: usize,
}

impl PsramPoolAdmission {
    pub const fn new(
        role: PsramPoolRole,
        enabled: bool,
        pressure: PressureLevel,
        budget_bytes: usize,
    ) -> Self {
        Self {
            role,
            enabled,
            pressure,
            budget_bytes,
        }
    }

    pub fn should_hold_pool(self) -> bool {
        self.enabled && self.pressure == PressureLevel::Normal && self.budget_bytes > 0
    }
}

/// Small RAII-owned cache pool. The buffer is reusable scratch; clearing drops the allocation.
#[derive(Debug)]
pub struct ModePsramPool {
    role: PsramPoolRole,
    bytes: Option<ByteBuffer>,
}

impl ModePsramPool {
    pub const fn new(role: PsramPoolRole) -> Self {
        Self { role, bytes: None }
    }

    pub fn role(&self) -> PsramPoolRole {
        self.role
    }

    pub fn held_bytes(&self) -> usize {
        self.bytes.as_ref().map(ByteBuffer::len).unwrap_or(0)
    }

    pub fn is_held(&self) -> bool {
        self.bytes.is_some()
    }

    /// Reconcile this pool with the latest pressure/mode admission.
    pub fn reconcile(&mut self, admission: PsramPoolAdmission) {
        debug_assert_eq!(self.role, admission.role);
        if !admission.should_hold_pool() {
            self.bytes = None;
            return;
        }
        let needs_alloc = self
            .bytes
            .as_ref()
            .map(|bytes| bytes.len() != admission.budget_bytes)
            .unwrap_or(true);
        if needs_alloc {
            // `new` gives a real fixed-size scratch buffer; on ESP large buffers prefer PSRAM.
            self.bytes = Some(ByteBuffer::zeroed(admission.budget_bytes));
        }
    }
}

/// Display asset cache budget is intentionally bounded and mode-gated.
pub fn display_asset_cache_budget_bytes(width: u16, height: u16) -> usize {
    let frame_bytes = width as usize * height as usize * 2;
    (frame_bytes / 4).clamp(16 * 1024, 128 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_holds_only_when_enabled_and_pressure_normal() {
        let mut pool = ModePsramPool::new(PsramPoolRole::DisplayAssetCache);
        pool.reconcile(PsramPoolAdmission::new(
            PsramPoolRole::DisplayAssetCache,
            true,
            PressureLevel::Normal,
            16 * 1024,
        ));
        assert!(pool.is_held());
        assert_eq!(pool.held_bytes(), 16 * 1024);

        pool.reconcile(PsramPoolAdmission::new(
            PsramPoolRole::DisplayAssetCache,
            true,
            PressureLevel::Cautious,
            16 * 1024,
        ));
        assert!(!pool.is_held());
    }

    #[test]
    fn display_asset_budget_is_bounded() {
        assert_eq!(display_asset_cache_budget_bytes(64, 64), 16 * 1024);
        assert_eq!(display_asset_cache_budget_bytes(800, 480), 128 * 1024);
    }
}
