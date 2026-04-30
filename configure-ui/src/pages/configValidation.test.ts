import assert from "node:assert/strict";
import test from "node:test";
import type { SystemConfigSegment } from "../types/appConfig.ts";
import {
  defaultDisplayConfig,
  type DisplayConfig,
} from "../types/displayConfig.ts";
import {
  defaultHardwareSegment,
  type HardwareSegment,
} from "../types/hardwareConfig.ts";
import {
  defaultAudioConfig,
  type AudioConfig,
} from "../types/audioConfig.ts";
import { validateSystemConfig } from "./systemConfigValidation.ts";
import { validateDisplayConfigForRuntime } from "./displayConfigValidation.ts";
import { validateHardwareSegment } from "./hardwareConfigValidation.ts";
import {
  mergeGpioHardwareDevices,
  mergeI2cSensorConfig,
} from "./hardwareSegmentDraft.ts";
import { validateAudioConfig } from "./audioConfigValidation.ts";

function t(key: string) {
  return key;
}

function createSystemConfig(): SystemConfigSegment {
  return {
    wifi_ssid: "",
    wifi_pass: "",
    proxy_url: "",
    locale: "zh",
  };
}

test("validateSystemConfig requires an SSID when a Wi-Fi password is present", () => {
  const form = createSystemConfig();
  form.wifi_pass = "secret";

  assert.equal(validateSystemConfig(form, t), "config.validation.wifiSsidRequired");
});

test("validateDisplayConfigForRuntime rejects framebuffer paths with control characters", () => {
  const form: DisplayConfig = {
    ...defaultDisplayConfig(),
    enabled: true,
    driver: "framebuffer",
    fb_device: "/dev/\u0007fb0",
  };

  assert.equal(
    validateDisplayConfigForRuntime(form, "linux", t),
    "displayConfig.validation.pathInvalid",
  );
});

test("validateHardwareSegment rejects duplicate GPIO pins", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    hardware_devices: [
      {
        id: "lamp",
        device_type: "gpio_out",
        pins: { pin: 4 },
        what: "",
        how: "",
        options: {},
      },
      {
        id: "fan",
        device_type: "gpio_out",
        pins: { pin: 4 },
        what: "",
        how: "",
        options: {},
      },
    ],
  };

  assert.equal(
    validateHardwareSegment(segment, t),
    "hardwareConfig.validation.pinDup",
  );
});

test("validateHardwareSegment requires i2c bus when i2c sensors exist", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: null,
    i2c_sensors: [
      {
        id: "box_aht20",
        addr: 0x38,
        model: "aht20",
        watch_field: "temperature",
        what: "",
        how: "",
        options: {},
      },
    ],
  };

  assert.equal(
    validateHardwareSegment(segment, t),
    "hardwareConfig.validation.i2cBusRequired",
  );
});

test("validateHardwareSegment rejects invalid i2c bus pins", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: { sda_pin: 41, scl_pin: 41, freq_hz: 100000 },
    i2c_sensors: [],
  };

  assert.equal(
    validateHardwareSegment(segment, t),
    "hardwareConfig.validation.i2cBusPinsDistinct",
  );
});

test("validateHardwareSegment rejects out-of-range i2c bus pins", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: { sda_pin: 49, scl_pin: 40, freq_hz: 100000 },
    i2c_sensors: [],
  };

  assert.equal(
    validateHardwareSegment(segment, t),
    "hardwareConfig.validation.i2cBusSdaPin",
  );
});

test("validateHardwareSegment accepts AHT20 i2c sensor config", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: { sda_pin: 21, scl_pin: 22, freq_hz: 100000 },
    i2c_sensors: [
      {
        id: "box_aht20",
        addr: 0x38,
        model: "aht20",
        watch_field: "temperature",
        what: "AHT20 temperature and humidity sensor",
        how: "AHT20 on I2C expansion bus",
        options: {},
      },
    ],
  };

  assert.equal(validateHardwareSegment(segment, t), null);
});

test("hardware segment merge helpers preserve unrelated fields", () => {
  const base: HardwareSegment = {
    hardware_devices: [
      {
        id: "relay",
        device_type: "gpio_out",
        pins: { pin: 4 },
        what: "relay",
        how: "toggle relay",
        options: {},
      },
    ],
    i2c_bus: { sda_pin: 41, scl_pin: 40, freq_hz: 100000 },
    i2c_devices: [{ id: "codec", addr: 0x18, what: "codec", how: "raw", options: {} }],
    i2c_sensors: [
      {
        id: "box_aht20",
        addr: 0x38,
        model: "aht20",
        watch_field: "humidity",
        what: "sensor",
        how: "read env",
        options: {},
      },
    ],
  };

  const gpioOnly = mergeGpioHardwareDevices(base, []);
  assert.deepEqual(gpioOnly.i2c_bus, base.i2c_bus);
  assert.deepEqual(gpioOnly.i2c_devices, base.i2c_devices);
  assert.deepEqual(gpioOnly.i2c_sensors, base.i2c_sensors);

  const i2cOnly = mergeI2cSensorConfig(base, null, []);
  assert.deepEqual(i2cOnly.hardware_devices, base.hardware_devices);
  assert.deepEqual(i2cOnly.i2c_devices, base.i2c_devices);
  assert.equal(i2cOnly.i2c_bus, null);
  assert.deepEqual(i2cOnly.i2c_sensors, []);
});

test("validateAudioConfig rejects microphone capture on linux runtime", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    microphone: {
      ...defaultAudioConfig().microphone,
      enabled: true,
    },
  };

  assert.equal(
    validateAudioConfig(form, "linux", t),
    "audioConfig.validation.linuxMicrophoneUnsupported",
  );
});
