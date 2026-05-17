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
        what: "AHT20 temperature and humidity sensor",
        how: "AHT20 on I2C expansion bus",
        options: {},
      },
    ],
  };

  assert.equal(validateHardwareSegment(segment, t), null);
});

test("validateHardwareSegment accepts valid i2s bus config", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2s_bus: {
      mclk_pin: 2,
      ws_pin: 47,
      bclk_pin: 17,
      din_pin: 16,
      dout_pin: 15,
    },
  };

  assert.equal(validateHardwareSegment(segment, t), null);
});

test("validateHardwareSegment rejects duplicate i2s bus pins", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2s_bus: {
      mclk_pin: 2,
      ws_pin: 47,
      bclk_pin: 17,
      din_pin: 17,
      dout_pin: 15,
    },
  };

  assert.equal(
    validateHardwareSegment(segment, t),
    "hardwareConfig.validation.i2sBusPinsDistinct",
  );
});

test("validateHardwareSegment requires i2c and i2s bus for i2s codec topology", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: null,
    i2s_bus: null,
  };
  const audio = {
    ...defaultAudioConfig(),
    topology: "i2s_codec" as const,
  };

  assert.equal(
    validateHardwareSegment(segment, t, audio),
    "hardwareConfig.validation.codecI2cBusRequired",
  );
});

test("validateHardwareSegment requires i2s bus for i2s codec topology", () => {
  const segment: HardwareSegment = {
    ...defaultHardwareSegment(),
    i2c_bus: { sda_pin: 21, scl_pin: 22 },
    i2s_bus: null,
  };
  const audio = {
    ...defaultAudioConfig(),
    topology: "i2s_codec" as const,
  };

  assert.equal(
    validateHardwareSegment(segment, t, audio),
    "hardwareConfig.validation.codecI2sBusRequired",
  );
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

test("validateAudioConfig rejects i2s codec topology on linux runtime", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    topology: "i2s_codec",
  };

  assert.equal(
    validateAudioConfig(form, "linux", t, defaultHardwareSegment()),
    "audioConfig.validation.linuxCodecTopologyUnsupported",
  );
});

test("validateAudioConfig requires codec fields for i2s codec topology", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    enabled: true,
    topology: "i2s_codec",
    microphone: {
      ...defaultAudioConfig().microphone,
      enabled: true,
    },
    speaker: {
      ...defaultAudioConfig().speaker,
      enabled: true,
    },
  };

  assert.equal(
    validateAudioConfig(form, "esp", t, {
      ...defaultHardwareSegment(),
      i2c_bus: { sda_pin: 21, scl_pin: 22, freq_hz: 100000 },
      i2s_bus: {
        mclk_pin: 2,
        ws_pin: 47,
        bclk_pin: 17,
        din_pin: 16,
        dout_pin: 15,
      },
    }),
    "audioConfig.validation.codecInputCodecRequired",
  );
});

test("validateAudioConfig skips legacy pin checks for i2s codec topology", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    enabled: true,
    topology: "i2s_codec",
    microphone: {
      ...defaultAudioConfig().microphone,
      enabled: true,
      pins: { ws: 0, sck: 0, din: 0 },
    },
    speaker: {
      ...defaultAudioConfig().speaker,
      enabled: true,
      pins: { ws: 0, sck: 0, dout: 0, sd: null },
    },
    codec: {
      input_codec: "es7210",
      output_codec: "es8311",
      input_addr: null,
      output_addr: null,
      pa_pin: 5,
      input_reference: false,
    },
  };

  assert.equal(
    validateAudioConfig(form, "esp", t, {
      ...defaultHardwareSegment(),
      i2c_bus: { sda_pin: 21, scl_pin: 22, freq_hz: 100000 },
      i2s_bus: {
        mclk_pin: 2,
        ws_pin: 47,
        bclk_pin: 17,
        din_pin: 16,
        dout_pin: 15,
      },
    }),
    null,
  );
});

test("validateAudioConfig requires hardware buses for i2s codec topology", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    enabled: true,
    topology: "i2s_codec",
    codec: {
      input_codec: "es7210",
      output_codec: "es8311",
      input_addr: null,
      output_addr: null,
      pa_pin: 5,
      input_reference: false,
    },
  };

  assert.equal(
    validateAudioConfig(form, "esp", t, defaultHardwareSegment()),
    "audioConfig.validation.codecI2cBusRequired",
  );
});

test("validateAudioConfig requires equal mic and speaker sample rates for i2s codec topology", () => {
  const form: AudioConfig = {
    ...defaultAudioConfig(),
    enabled: true,
    topology: "i2s_codec",
    microphone: {
      ...defaultAudioConfig().microphone,
      enabled: true,
      sample_rate: 16000,
    },
    speaker: {
      ...defaultAudioConfig().speaker,
      enabled: true,
      sample_rate: 24000,
    },
    codec: {
      input_codec: "es7210",
      output_codec: "es8311",
      input_addr: null,
      output_addr: null,
      pa_pin: 5,
      input_reference: false,
    },
  };

  assert.equal(
    validateAudioConfig(form, "esp", t, {
      ...defaultHardwareSegment(),
      i2c_bus: { sda_pin: 21, scl_pin: 22, freq_hz: 100000 },
      i2s_bus: {
        mclk_pin: 2,
        ws_pin: 47,
        bclk_pin: 17,
        din_pin: 16,
        dout_pin: 15,
      },
    }),
    "audioConfig.validation.codecSampleRateMatch",
  );
});
