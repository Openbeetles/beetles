import type {
  DeviceEntry,
  HardwareSegment,
  I2cBusConfig,
  I2cSensorEntry,
} from "../types/hardwareConfig.ts";
import { defaultHardwareSegment } from "../types/hardwareConfig.ts";

export function mergeGpioHardwareDevices(
  base: HardwareSegment | null,
  devices: DeviceEntry[],
): HardwareSegment {
  return {
    ...(base ?? defaultHardwareSegment()),
    hardware_devices: devices,
  };
}

export function mergeI2cSensorConfig(
  base: HardwareSegment | null,
  i2cBus: I2cBusConfig | null,
  i2cSensors: I2cSensorEntry[],
): HardwareSegment {
  return {
    ...(base ?? defaultHardwareSegment()),
    i2c_bus: i2cBus,
    i2c_sensors: i2cSensors,
  };
}
