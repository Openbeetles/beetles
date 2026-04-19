import type { AudioConfig } from "../types/audioConfig.ts";
import {
  AUDIO_BITS_PER_SAMPLE_ALLOWED,
  AUDIO_BUFFER_SIZE_MAX,
  AUDIO_BUFFER_SIZE_MIN,
  AUDIO_CONFIG_VERSION,
  AUDIO_PIN_MAX,
  AUDIO_PIN_MIN,
  AUDIO_SAMPLE_RATE_MAX,
  AUDIO_SAMPLE_RATE_MIN,
  audioRealtimeConfigured,
  audioRealtimeProviderSupported,
  audioSpeakerPinsOrDefault,
  realtimeRequiredSampleRate,
} from "../types/audioConfig.ts";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore.ts";

export function validateAudioConfig(
  form: AudioConfig,
  runtimeKind: DeviceRuntimeKind,
  t: (key: string) => string,
): string | null {
  const realtimeReady = audioRealtimeConfigured(form);
  const wakeVoicePipelineEnabled = form.enabled && form.wake_word.enabled;
  const speakerPins = audioSpeakerPinsOrDefault(form.speaker.pins);

  if (form.version !== AUDIO_CONFIG_VERSION) {
    return t("audioConfig.validation.version");
  }

  const pinInRange = (pin: number) => pin >= AUDIO_PIN_MIN && pin <= AUDIO_PIN_MAX;
  const sampleRateInRange = (value: number) =>
    value >= AUDIO_SAMPLE_RATE_MIN && value <= AUDIO_SAMPLE_RATE_MAX;
  const bitsPerSampleValid = (value: number) =>
    (AUDIO_BITS_PER_SAMPLE_ALLOWED as readonly number[]).includes(value);

  if (runtimeKind === "linux" && form.microphone.enabled) {
    return t("audioConfig.validation.linuxMicrophoneUnsupported");
  }

  if (form.microphone.enabled) {
    if (
      !pinInRange(form.microphone.pins.ws) ||
      !pinInRange(form.microphone.pins.sck) ||
      !pinInRange(form.microphone.pins.din)
    ) {
      return t("audioConfig.validation.pin");
    }
    if (!sampleRateInRange(form.microphone.sample_rate)) {
      return t("audioConfig.validation.sampleRate");
    }
    if (!bitsPerSampleValid(form.microphone.bits_per_sample)) {
      return t("audioConfig.validation.bitsPerSample");
    }
    if (
      form.microphone.buffer_size < AUDIO_BUFFER_SIZE_MIN ||
      form.microphone.buffer_size > AUDIO_BUFFER_SIZE_MAX
    ) {
      return t("audioConfig.validation.bufferSize");
    }
  }

  if (form.speaker.enabled) {
    if (runtimeKind === "linux") {
      if (form.speaker.device_type !== "usb") {
        return t("audioConfig.validation.linuxSpeakerDeviceType");
      }
      if (!form.speaker.device_ref?.trim()) {
        return t("audioConfig.validation.speakerUsbRequired");
      }
    } else {
      if (
        !pinInRange(speakerPins.ws) ||
        !pinInRange(speakerPins.sck) ||
        !pinInRange(speakerPins.dout)
      ) {
        return t("audioConfig.validation.pin");
      }
      if (speakerPins.sd != null && !pinInRange(speakerPins.sd)) {
        return t("audioConfig.validation.pin");
      }
    }
    if (!sampleRateInRange(form.speaker.sample_rate)) {
      return t("audioConfig.validation.sampleRate");
    }
    if (!bitsPerSampleValid(form.speaker.bits_per_sample)) {
      return t("audioConfig.validation.bitsPerSample");
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
