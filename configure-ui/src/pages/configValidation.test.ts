import assert from "node:assert/strict";
import test from "node:test";
import type { AppConfig } from "../types/appConfig.ts";
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

function createSystemConfig(): AppConfig {
  return {
    wifi_ssid: "",
    wifi_pass: "",
    tg_token: "",
    tg_allowed_chat_ids: "",
    feishu_app_id: "",
    feishu_app_secret: "",
    feishu_allowed_chat_ids: "",
    dingtalk_webhook_url: "",
    wecom_corp_id: "",
    wecom_corp_secret: "",
    wecom_agent_id: "",
    wecom_default_touser: "",
    qq_channel_app_id: "",
    qq_channel_secret: "",
    api_key: "",
    model: "",
    model_provider: "",
    api_url: "",
    proxy_url: "",
    search_key: "",
    tavily_key: "",
    tg_group_activation: "",
    session_max_messages: 12,
    webhook_enabled: false,
    webhook_token: "",
    enabled_channel: "",
    llm_sources: [],
    llm_router_source_index: null,
    llm_worker_source_index: null,
    llm_stream: false,
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
