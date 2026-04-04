//! GET /api/hardware/discovery：按 bus + capability 过滤设备发现结果。

use super::HandlerContext;
use crate::error::Error;
use crate::platform::{HardwareCapability, HardwareDiscoveryBus, HardwareDiscoveryQuery};

#[derive(Debug)]
pub enum HardwareDiscoveryError {
    Unavailable,
    Other(Error),
}

pub fn get_body(
    ctx: &HandlerContext,
    bus: HardwareDiscoveryBus,
    capability: HardwareCapability,
) -> Result<String, HardwareDiscoveryError> {
    let discovery = ctx
        .platform
        .hardware_discovery()
        .ok_or(HardwareDiscoveryError::Unavailable)?;
    let response = discovery
        .discover(&HardwareDiscoveryQuery { bus, capability })
        .map_err(HardwareDiscoveryError::Other)?;
    serde_json::to_string(&response).map_err(|error| {
        HardwareDiscoveryError::Other(Error::Other {
            source: Box::new(error),
            stage: "hardware_discovery_serialize",
        })
    })
}
