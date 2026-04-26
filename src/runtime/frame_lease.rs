//! Borrowed camera frame lease guard.
//! 借用式 camera frame 租约守卫；只治理 frame 生命周期，不接入硬件采集。

use crate::orchestrator::PressureLevel;
use crate::runtime::lease::{LeaseDecision, LeaseKind, LeaseOwner, LeaseReplacePolicy};
use crate::runtime::{RuntimeMode, RuntimeModeSnapshot};
use crate::{Error, Result};

/// Camera frame admission snapshot.
///
/// Capture code should evaluate this before allocating/capturing a large frame,
/// then hold a [`FrameLease`] while the borrowed frame is consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLeaseAdmission {
    pub runtime_mode: RuntimeModeSnapshot,
    pub pressure: PressureLevel,
}

impl FrameLeaseAdmission {
    /// Build admission from the current runtime mode and latest resource pressure.
    pub fn current() -> Self {
        Self {
            runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
            pressure: crate::orchestrator::refresh_heap_if_stale(),
        }
    }

    fn denial_reason(self) -> Option<&'static str> {
        if self.pressure == PressureLevel::Critical {
            return Some("critical_pressure_camera_frame");
        }
        match self.runtime_mode.current_mode {
            RuntimeMode::Normal => None,
            RuntimeMode::Booting
            | RuntimeMode::Pairing
            | RuntimeMode::ConfigActive
            | RuntimeMode::VoiceExclusive
            | RuntimeMode::Maintenance
            | RuntimeMode::RecoverySafeMode => self
                .runtime_mode
                .mode_block_reason()
                .or(Some("runtime_mode_camera_frame_blocked")),
        }
    }

    /// Return whether camera frame capture may start under this snapshot.
    pub fn ensure_allowed(self) -> Result<()> {
        match self.denial_reason() {
            Some(reason) => Err(Error::config("camera_frame_admission", reason)),
            None => Ok(()),
        }
    }
}

/// RAII guard for a single borrowed camera frame.
pub struct FrameLease<'a> {
    data: &'a [u8],
    owner: LeaseOwner,
    token: u64,
}

impl<'a> FrameLease<'a> {
    /// Return the borrowed frame bytes without copying.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}

impl Drop for FrameLease<'_> {
    fn drop(&mut self) {
        let _ =
            crate::runtime::lease::release_token(LeaseKind::CameraFrame, self.owner, self.token);
    }
}

/// Try to borrow one camera frame under the central runtime lease registry.
pub fn try_borrow_frame<'a>(owner: LeaseOwner, data: &'a [u8]) -> Result<FrameLease<'a>> {
    let admission = admit_current_camera_frame_capture()?;
    try_borrow_frame_with_admission(owner, data, admission)
}

/// Admit a camera frame capture before allocating/capturing a large frame.
pub fn admit_current_camera_frame_capture() -> Result<FrameLeaseAdmission> {
    let admission = FrameLeaseAdmission::current();
    admission.ensure_allowed()?;
    Ok(admission)
}

/// Try to borrow one camera frame after an explicit mode/pressure admission snapshot.
pub fn try_borrow_frame_with_admission<'a>(
    owner: LeaseOwner,
    data: &'a [u8],
    admission: FrameLeaseAdmission,
) -> Result<FrameLease<'a>> {
    try_borrow_frame_with_admission_inner(owner, data, admission, None)
}

#[cfg(test)]
fn try_borrow_frame_with_admission_at<'a>(
    owner: LeaseOwner,
    data: &'a [u8],
    admission: FrameLeaseAdmission,
    now_ms: u64,
) -> Result<FrameLease<'a>> {
    try_borrow_frame_with_admission_inner(owner, data, admission, Some(now_ms))
}

fn try_borrow_frame_with_admission_inner<'a>(
    owner: LeaseOwner,
    data: &'a [u8],
    admission: FrameLeaseAdmission,
    now_ms: Option<u64>,
) -> Result<FrameLease<'a>> {
    admission.ensure_allowed()?;

    let decision = match now_ms {
        Some(now_ms) => crate::runtime::lease::try_acquire_exclusive_once_at(
            LeaseKind::CameraFrame,
            owner,
            None,
            now_ms,
            LeaseReplacePolicy::Never,
        ),
        None => crate::runtime::lease::try_acquire_exclusive_once(
            LeaseKind::CameraFrame,
            owner,
            None,
            LeaseReplacePolicy::Never,
        ),
    };

    match decision {
        LeaseDecision::Acquired(record) => Ok(FrameLease {
            data,
            owner,
            token: record.token,
        }),
        LeaseDecision::Denied(denial) => Err(Error::config(
            "camera_frame_lease",
            format!(
                "camera frame lease denied reason={} held_by={:?}",
                denial.reason, denial.held_by
            ),
        )),
        LeaseDecision::Reentered(_) | LeaseDecision::ReplacedExpired { .. } => Err(Error::config(
            "camera_frame_lease",
            "unexpected camera frame lease decision",
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::lease::{self, LeaseKind, LeaseOwner};
    use crate::runtime::mode::RuntimeModeSource;
    use crate::runtime::RuntimeMode;

    fn admission_for(mode: RuntimeMode, critical_pressure: bool) -> super::FrameLeaseAdmission {
        let mut source = RuntimeModeSource::default();
        match mode {
            RuntimeMode::Booting => source.boot_phase_active = true,
            RuntimeMode::Pairing => {
                source.pairing_state_known = true;
                source.pairing_required = true;
            }
            RuntimeMode::Normal => {}
            RuntimeMode::ConfigActive => source.config_active = true,
            RuntimeMode::VoiceExclusive => source.voice_exclusive_active = true,
            RuntimeMode::Maintenance => source.background_maintenance_active = true,
            RuntimeMode::RecoverySafeMode => source.recovery_safe_mode_active = true,
        }
        super::FrameLeaseAdmission {
            runtime_mode: crate::runtime::mode::snapshot_from_source(source),
            pressure: if critical_pressure {
                crate::orchestrator::PressureLevel::Critical
            } else {
                crate::orchestrator::PressureLevel::Normal
            },
        }
    }

    #[test]
    fn frame_lease_is_single_borrow_and_reacquires_after_drop() {
        let _guard = lease::lease_test_guard();
        let owner = LeaseOwner::new("camera", "capture");
        let frame = [1_u8, 2, 3, 4];

        let first = super::try_borrow_frame_with_admission_at(
            owner,
            &frame,
            admission_for(RuntimeMode::Normal, false),
            100,
        )
        .expect("first frame lease");
        assert_eq!(first.as_bytes(), frame.as_slice());
        assert!(std::ptr::eq(first.as_bytes().as_ptr(), frame.as_ptr()));
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 101),
            1
        );

        assert!(super::try_borrow_frame_with_admission_at(
            owner,
            &frame,
            admission_for(RuntimeMode::Normal, false),
            102,
        )
        .is_err());
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 103),
            1
        );

        drop(first);
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 104),
            0
        );

        let second = super::try_borrow_frame_with_admission_at(
            owner,
            &frame,
            admission_for(RuntimeMode::Normal, false),
            105,
        )
        .expect("second frame lease");
        assert!(std::ptr::eq(second.as_bytes().as_ptr(), frame.as_ptr()));
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 106),
            1
        );
    }

    #[test]
    fn frame_lease_admission_denies_critical_pressure_before_borrow() {
        let _guard = lease::lease_test_guard();
        let owner = LeaseOwner::new("camera", "capture");
        let frame = [1_u8, 2, 3, 4];

        let err = match super::try_borrow_frame_with_admission_at(
            owner,
            &frame,
            admission_for(RuntimeMode::Normal, true),
            200,
        ) {
            Ok(_) => panic!("critical pressure should deny camera frame admission"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("critical_pressure_camera_frame"));
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 201),
            0
        );
    }

    #[test]
    fn frame_lease_admission_denies_voice_exclusive_before_borrow() {
        let _guard = lease::lease_test_guard();
        let owner = LeaseOwner::new("camera", "capture");
        let frame = [1_u8, 2, 3, 4];

        let err = match super::try_borrow_frame_with_admission_at(
            owner,
            &frame,
            admission_for(RuntimeMode::VoiceExclusive, false),
            300,
        ) {
            Ok(_) => panic!("voice exclusive mode should deny camera frame admission"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("voice_exclusive_active"));
        assert_eq!(
            lease::active_count_for_kind_at(LeaseKind::CameraFrame, 301),
            0
        );
    }
}
