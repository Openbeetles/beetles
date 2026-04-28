//! Platform camera capture contract.
//! 平台 camera 采集合同；只表达真实硬件状态与真实帧字节，不提供假采集。

use crate::platform::ByteBuffer;
use crate::{Error, Result};
use std::sync::Arc;

const CAMERA_CAPTURE_STAGE: &str = "camera_capture";

/// Current platform camera capability state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraState {
    Unavailable,
    Available,
}

/// Camera frame encoding produced by the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraFrameFormat {
    Jpeg,
    Png,
    Rgb888,
    Grayscale8,
}

impl CameraFrameFormat {
    /// MIME type used when forwarding encoded frames to vision providers.
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Rgb888 => "application/octet-stream",
            Self::Grayscale8 => "application/octet-stream",
        }
    }
}

/// Platform camera status. `Available` must mean a real capture path exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CameraStatus {
    pub state: CameraState,
    pub label: &'static str,
    pub max_frame_bytes: Option<usize>,
    pub formats: &'static [CameraFrameFormat],
}

impl CameraStatus {
    pub const fn unavailable(label: &'static str) -> Self {
        Self {
            state: CameraState::Unavailable,
            label,
            max_frame_bytes: None,
            formats: &[],
        }
    }

    pub const fn is_available(self) -> bool {
        matches!(self.state, CameraState::Available)
    }
}

/// Owned camera frame bytes plus capture metadata.
#[derive(Debug)]
pub struct CameraFrameBuffer {
    pub bytes: ByteBuffer,
    pub format: CameraFrameFormat,
    pub width: u32,
    pub height: u32,
    pub captured_at_ms: u64,
    pub owner_label: &'static str,
}

/// Platform-owned camera capture provider.
pub trait PlatformCamera: Send + Sync {
    fn camera_status(&self) -> CameraStatus {
        CameraStatus::unavailable("camera_unavailable")
    }

    /// Capture one real frame.
    ///
    /// Callers must acquire the runtime camera frame admission/lease before
    /// invoking this method. The platform implementation enforces hardware
    /// truth and byte limits; it does not own cross-plane admission.
    fn capture_frame(&self, _max_bytes: usize) -> Result<CameraFrameBuffer> {
        Err(Error::config(CAMERA_CAPTURE_STAGE, "camera_unavailable"))
    }
}

impl<T: PlatformCamera + ?Sized> PlatformCamera for Arc<T> {
    fn camera_status(&self) -> CameraStatus {
        (**self).camera_status()
    }

    fn capture_frame(&self, max_bytes: usize) -> Result<CameraFrameBuffer> {
        (**self).capture_frame(max_bytes)
    }
}
