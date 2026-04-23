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
import { validateAudioConfig } from "./audioConfigValidation.ts";

function t(key: string) {
  return key;
}

function createSystemConfig(): SystemConfigSegment {
  return {
    wifi_ssid: "",
    wifi_pass: "",
    proxy_url: "",
    tg_group_activation: "",
    session_max_messages: 12,
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
