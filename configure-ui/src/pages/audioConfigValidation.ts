import type { AudioConfig } from "../types/audioConfig.ts";
import type { HardwareSegment } from "../types/hardwareConfig.ts";
import {
  AUDIO_CONFIG_VERSION,
  AUDIO_I2C_ADDR_MAX,
  AUDIO_I2C_ADDR_MIN,
  AUDIO_PIN_MAX,
  AUDIO_PIN_MIN,
  AUDIO_SAMPLE_RATE_MAX,
  AUDIO_SAMPLE_RATE_MIN,
  audioInputCodecSupported,
  audioOutputCodecSupported,
  audioRealtimeConfigured,
  audioRealtimeProviderSupported,
  audioSpeakerPinsOrDefault,
  audioTopologySupported,
  realtimeRequiredSampleRate,
} from "../types/audioConfig.ts";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore.ts";

export function validateAudioConfig(
  form: AudioConfig,
  runtimeKind: DeviceRuntimeKind,
  t: (key: string) => string,
  hardwareSegment?: HardwareSegment | null,
): string | null {
  const realtimeReady = audioRealtimeConfigured(form);
  const wakeVoicePipelineEnabled = form.enabled && form.wake_word.enabled;
  const speakerPins = audioSpeakerPinsOrDefault(form.speaker.pins);
  const wake = form.wake_word;
  const codecTopology = form.topology === "i2s_codec";

  if (form.version !== AUDIO_CONFIG_VERSION) {
    return t("audioConfig.validation.version");
  }
  if (!audioTopologySupported(form.topology)) {
    return t("audioConfig.validation.topology");
  }

  const pinInRange = (pin: number) => pin >= AUDIO_PIN_MIN && pin <= AUDIO_PIN_MAX;
  const sampleRateInRange = (value: number) =>
    value >= AUDIO_SAMPLE_RATE_MIN && value <= AUDIO_SAMPLE_RATE_MAX;
  const addrInRange = (value: number) =>
    value >= AUDIO_I2C_ADDR_MIN && value <= AUDIO_I2C_ADDR_MAX;

  if (runtimeKind === "linux" && form.microphone.enabled) {
    return t("audioConfig.validation.linuxMicrophoneUnsupported");
  }
  if (runtimeKind === "linux" && codecTopology) {
    return t("audioConfig.validation.linuxCodecTopologyUnsupported");
  }

  if (form.microphone.enabled) {
    if (
      !codecTopology &&
      (
        !pinInRange(form.microphone.pins.ws) ||
        !pinInRange(form.microphone.pins.sck) ||
        !pinInRange(form.microphone.pins.din)
      )
    ) {
      return t("audioConfig.validation.pin");
    }
    if (!sampleRateInRange(form.microphone.sample_rate)) {
      return t("audioConfig.validation.sampleRate");
    }
  }

  if (form.speaker.enabled) {
    if (runtimeKind === "linux") {
      if (!codecTopology && form.speaker.device_type !== "usb") {
        return t("audioConfig.validation.linuxSpeakerDeviceType");
      }
      if (!codecTopology && !form.speaker.device_ref?.trim()) {
        return t("audioConfig.validation.speakerUsbRequired");
      }
    } else if (
      !codecTopology &&
      (
        !pinInRange(speakerPins.ws) ||
        !pinInRange(speakerPins.sck) ||
        !pinInRange(speakerPins.dout)
      )
    ) {
      return t("audioConfig.validation.pin");
    } else if (
      !codecTopology &&
      speakerPins.sd != null &&
      !pinInRange(speakerPins.sd)
    ) {
      return t("audioConfig.validation.pin");
    }
    if (!sampleRateInRange(form.speaker.sample_rate)) {
      return t("audioConfig.validation.sampleRate");
    }
  }

  if (codecTopology) {
    if (hardwareSegment?.i2c_bus == null) {
      return t("audioConfig.validation.codecI2cBusRequired");
    }
    if (hardwareSegment.i2s_bus == null) {
      return t("audioConfig.validation.codecI2sBusRequired");
    }
    if (!form.codec.input_codec?.trim()) {
      return t("audioConfig.validation.codecInputCodecRequired");
    }
    if (!audioInputCodecSupported(form.codec.input_codec.trim())) {
      return t("audioConfig.validation.codecInputCodec");
    }
    if (!form.codec.output_codec?.trim()) {
      return t("audioConfig.validation.codecOutputCodecRequired");
    }
    if (!audioOutputCodecSupported(form.codec.output_codec.trim())) {
      return t("audioConfig.validation.codecOutputCodec");
    }
    if (form.codec.input_addr != null && !addrInRange(form.codec.input_addr)) {
      return t("audioConfig.validation.codecInputAddr");
    }
    if (form.codec.output_addr != null && !addrInRange(form.codec.output_addr)) {
      return t("audioConfig.validation.codecOutputAddr");
    }
    if (form.codec.pa_pin == null || !pinInRange(form.codec.pa_pin)) {
      return t("audioConfig.validation.codecPaPin");
    }
    if (typeof form.codec.input_reference !== "boolean") {
      return t("audioConfig.validation.codecInputReference");
    }
    if (
      form.microphone.enabled &&
      form.speaker.enabled &&
      form.microphone.sample_rate !== form.speaker.sample_rate
    ) {
      return t("audioConfig.validation.codecSampleRateMatch");
    }
  }

  if (form.vad.threshold < 0 || form.vad.threshold > 1) {
    return t("audioConfig.validation.vadThreshold");
  }
  if (
    form.vad.silence_duration_ms < 1 ||
    form.vad.silence_duration_ms > 60_000
  ) {
    return t("audioConfig.validation.vadSilence");
  }

  if (form.ambient_listening.sound_events.length > 16) {
    return t("audioConfig.validation.soundEvents");
  }
  if (
    form.ambient_listening.sound_events.some(
      (sound) => !sound.trim() || sound.length > 32,
    )
  ) {
    return t("audioConfig.validation.soundEvents");
  }
  if (
    form.ambient_listening.check_interval_seconds < 1 ||
    form.ambient_listening.check_interval_seconds > 86_400
  ) {
    return t("audioConfig.validation.checkInterval");
  }
  if (form.led_indicator.enabled && !pinInRange(form.led_indicator.pin)) {
    return t("audioConfig.validation.pin");
  }

  if (wake.enabled) {
    if (wake.enter_threshold < 0.01 || wake.enter_threshold > 1) {
      return t("audioConfig.validation.wakeEnterThreshold");
    }
    if (wake.leave_threshold < 0 || wake.leave_threshold > 1) {
      return t("audioConfig.validation.wakeLeaveThreshold");
    }
    if (wake.leave_threshold >= wake.enter_threshold) {
      return t("audioConfig.validation.wakeThresholdOrder");
    }
    if (wake.reference_suppress_ratio < 0.5 || wake.reference_suppress_ratio > 4) {
      return t("audioConfig.validation.wakeReferenceSuppressRatio");
    }
    if (
      wake.zcr_min < 0 ||
      wake.zcr_min > 1 ||
      wake.zcr_max < 0 ||
      wake.zcr_max > 1 ||
      wake.zcr_min >= wake.zcr_max
    ) {
      return t("audioConfig.validation.wakeZcrRange");
    }
    if (wake.min_speech_band_ratio < 0 || wake.min_speech_band_ratio > 1) {
      return t("audioConfig.validation.wakeSpeechBandRatio");
    }
    if (
      wake.min_active_ms < 20 ||
      wake.min_active_ms > 5_000 ||
      wake.hangover_ms < 0 ||
      wake.hangover_ms > 10_000 ||
      wake.cooldown_ms < 100 ||
      wake.cooldown_ms > 10_000
    ) {
      return t("audioConfig.validation.wakeTimingMs");
    }
  }

  if (wakeVoicePipelineEnabled && !form.microphone.enabled) {
    return t("audioConfig.validation.wakeWordMicRequired");
  }
  if (wakeVoicePipelineEnabled && !form.speaker.enabled) {
    return t("audioConfig.validation.wakeWordSpeakerRequired");
  }

  if (
    wakeVoicePipelineEnabled &&
    !realtimeReady &&
    form.service_provider !== "baidu"
  ) {
    return t("audioConfig.validation.wakeWordSpeechPipeline");
  }

  if (
    wakeVoicePipelineEnabled &&
    !realtimeReady &&
    form.microphone.enabled &&
    form.service_provider === "baidu" &&
    (!form.speech.api_key.trim() || !form.speech.api_secret.trim())
  ) {
    return t("audioConfig.validation.speechInputCredentialRequired");
  }

  if (
    wakeVoicePipelineEnabled &&
    !realtimeReady &&
    form.speaker.enabled &&
    form.service_provider === "baidu" &&
    (!form.speech.api_key.trim() || !form.speech.api_secret.trim())
  ) {
    return t("audioConfig.validation.speechOutputCredentialRequired");
  }

  if (wakeVoicePipelineEnabled && realtimeReady) {
    if (!form.microphone.enabled) {
      return t("audioConfig.validation.realtimeMicRequired");
    }
    if (!form.speaker.enabled) {
      return t("audioConfig.validation.realtimeSpeakerRequired");
    }
    if (!audioRealtimeProviderSupported(form.realtime.provider)) {
      return t("audioConfig.validation.realtimeProvider");
    }
    if (!form.realtime.api_key.trim()) {
      return t("audioConfig.validation.realtimeApiKey");
    }
    if (!form.realtime.model.trim()) {
      return t("audioConfig.validation.realtimeModel");
    }
    if (!form.realtime.voice.trim()) {
      return t("audioConfig.validation.realtimeVoice");
    }
    if (
      !form.realtime.ws_url.startsWith("wss://") &&
      !form.realtime.ws_url.startsWith("ws://")
    ) {
      return t("audioConfig.validation.realtimeWsUrl");
    }
    const requiredSampleRate = realtimeRequiredSampleRate(form.realtime.provider);
    if (
      form.microphone.sample_rate !== requiredSampleRate ||
      form.speaker.sample_rate !== requiredSampleRate
    ) {
      return t("audioConfig.validation.realtimeSampleRate");
    }
  }

  return null;
}
