export interface AudioMicPins {
  ws: number
  sck: number
  din: number
}

export interface AudioSpeakerPins {
  ws: number
  sck: number
  dout: number
  sd?: number | null
}

export interface AudioMicrophoneConfig {
  enabled: boolean
  device_type: string
  pins: AudioMicPins
  sample_rate: number
  bits_per_sample: number
  buffer_size: number
}

export interface AudioSpeakerConfig {
  enabled: boolean
  device_type: string
  pins: AudioSpeakerPins
  sample_rate: number
  bits_per_sample: number
}

export interface AudioVadConfig {
  threshold: number
  silence_duration_ms: number
}

export interface AudioWakeWordConfig {
  enabled: boolean
  keyword: string
  wake_prompt: string
}

export interface AudioSpeechConfig {
  api_url: string
  api_key: string
  api_secret: string
  model: string
  language: string
}

export interface AudioTtsConfig {
  voice: string
  rate: string
  pitch: string
}

export interface AudioRealtimeConfig {
  provider: string
  ws_url: string
  api_key: string
  api_secret: string
  app_id: string
  model: string
  voice: string
  instructions: string
  user_id: string
  license_key: string
  device_id: string
}

export interface AudioAmbientListeningConfig {
  enabled: boolean
  detect_emotions: boolean
  sound_events: string[]
  cooldown_minutes: number
  check_interval_seconds: number
}

export interface AudioLedStatesConfig {
  listening: string
  processing: string
  speaking: string
}

export interface AudioLedIndicatorConfig {
  enabled: boolean
  pin: number
  states: AudioLedStatesConfig
}

export interface AudioConfig {
  version: number
  enabled: boolean
  service_provider: string
  microphone: AudioMicrophoneConfig
  speaker: AudioSpeakerConfig
  vad: AudioVadConfig
  wake_word: AudioWakeWordConfig
  speech: AudioSpeechConfig
  tts: AudioTtsConfig
  realtime: AudioRealtimeConfig
  ambient_listening: AudioAmbientListeningConfig
  led_indicator: AudioLedIndicatorConfig
}

export const AUDIO_CONFIG_VERSION = 1
export const AUDIO_PIN_MIN = 1
export const AUDIO_PIN_MAX = 48
export const AUDIO_SAMPLE_RATE_MIN = 8_000
export const AUDIO_SAMPLE_RATE_MAX = 48_000
export const AUDIO_REALTIME_PCM16_SAMPLE_RATE = 24_000
export const AUDIO_REALTIME_BAIDU_SAMPLE_RATE = 16_000
export const AUDIO_BUFFER_SIZE_MIN = 256
export const AUDIO_BUFFER_SIZE_MAX = 16 * 1024
export const AUDIO_BITS_PER_SAMPLE_ALLOWED = [16, 24, 32] as const

export const AUDIO_MIC_DEVICE_TYPES = ['i2s_inmp441', 'pdm'] as const
export const AUDIO_SPEAKER_DEVICE_TYPES = ['i2s_max98357a'] as const

export const AUDIO_SAMPLE_RATE_PRESETS = [
  8_000,
  11_025,
  16_000,
  22_050,
  24_000,
  44_100,
  48_000,
] as const

export const AUDIO_BUFFER_PRESETS = [512, 1024, 2048, 4096, 8192] as const
export const AUDIO_VAD_THRESHOLD_PRESETS = [0.01, 0.02, 0.05, 0.08, 0.1, 0.2, 0.3, 0.5, 0.7] as const
export const AUDIO_VAD_SILENCE_MS_PRESETS = [500, 750, 1000, 1500, 2000, 3000] as const

export const AUDIO_SPEECH_PROVIDERS = ['baidu', 'whisper', 'xunfei'] as const
export const AUDIO_SPEECH_LANGUAGES = ['zh', 'en', 'ja', 'ko'] as const
export const AUDIO_REALTIME_PROVIDERS = ['openai_compatible', 'qwen', 'baidu'] as const

export type AudioRealtimeProvider = (typeof AUDIO_REALTIME_PROVIDERS)[number]

export const AUDIO_TTS_RATE_PRESETS = ['-20%', '-10%', '+0%', '+10%', '+20%'] as const
export const AUDIO_TTS_PITCH_PRESETS = ['-10Hz', '-5Hz', '+0Hz', '+5Hz', '+10Hz'] as const
export const AUDIO_LED_STATE_PRESETS = ['breathing', 'fast_blink', 'slow_blink', 'solid'] as const
export const AUDIO_AMBIENT_SOUND_EVENT_PRESETS = [
  'sigh',
  'cough',
  'laugh',
  'cry',
  'door_close',
] as const
export const AUDIO_AMBIENT_COOLDOWN_PRESETS = [5, 10, 15, 30, 60] as const
export const AUDIO_AMBIENT_CHECK_INTERVAL_PRESETS = [60, 120, 300, 600, 1800] as const

export const DEFAULT_SPEECH_API_URL_WHISPER = 'https://api.openai.com/v1/audio/transcriptions'
export const DEFAULT_SPEECH_API_URL_BAIDU = 'https://vop.baidu.com/server_api'
export const DEFAULT_REALTIME_PROVIDER: AudioRealtimeProvider = 'openai_compatible'
export const DEFAULT_REALTIME_WS_URL_OPENAI = 'wss://api.openai.com/v1/realtime'
export const DEFAULT_REALTIME_MODEL_OPENAI = 'gpt-realtime'
export const DEFAULT_REALTIME_VOICE_OPENAI = 'alloy'
export const DEFAULT_REALTIME_WS_URL_QWEN = 'wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime'
export const DEFAULT_REALTIME_MODEL_QWEN = 'qwen3.5-omni-plus-realtime'
export const DEFAULT_REALTIME_VOICE_QWEN = 'Cherry'
export const DEFAULT_REALTIME_WS_URL_BAIDU = 'wss://rtc-aiotgw.exp.bcelive.com/v1/realtime'

interface AudioRealtimeProviderDefaults {
  ws_url: string
  model: string
  voice: string
}

const AUDIO_REALTIME_PROVIDER_DEFAULTS: Record<AudioRealtimeProvider, AudioRealtimeProviderDefaults> = {
  openai_compatible: {
    ws_url: DEFAULT_REALTIME_WS_URL_OPENAI,
    model: DEFAULT_REALTIME_MODEL_OPENAI,
    voice: DEFAULT_REALTIME_VOICE_OPENAI,
  },
  qwen: {
    ws_url: DEFAULT_REALTIME_WS_URL_QWEN,
    model: DEFAULT_REALTIME_MODEL_QWEN,
    voice: DEFAULT_REALTIME_VOICE_QWEN,
  },
  baidu: {
    ws_url: DEFAULT_REALTIME_WS_URL_BAIDU,
    model: '',
    voice: '',
  },
}

export function audioRealtimeProviderSupported(provider: string): provider is AudioRealtimeProvider {
  return (AUDIO_REALTIME_PROVIDERS as readonly string[]).includes(provider)
}

export function realtimeProviderDefaults(provider: string): AudioRealtimeProviderDefaults {
  if (audioRealtimeProviderSupported(provider)) {
    return AUDIO_REALTIME_PROVIDER_DEFAULTS[provider]
  }
  return AUDIO_REALTIME_PROVIDER_DEFAULTS[DEFAULT_REALTIME_PROVIDER]
}

export function realtimeModelAfterProviderChange(
  currentModel: string,
  oldProvider: string,
  newProvider: string,
): string {
  const trimmed = currentModel.trim()
  if (!trimmed) {
    return realtimeProviderDefaults(newProvider).model
  }
  if (trimmed === realtimeProviderDefaults(oldProvider).model.trim()) {
    return realtimeProviderDefaults(newProvider).model
  }
  return currentModel
}

export function realtimeWsUrlAfterProviderChange(
  currentUrl: string,
  oldProvider: string,
  newProvider: string,
): string {
  const trimmed = currentUrl.trim()
  if (!trimmed) {
    return realtimeProviderDefaults(newProvider).ws_url
  }
  if (trimmed === realtimeProviderDefaults(oldProvider).ws_url.trim()) {
    return realtimeProviderDefaults(newProvider).ws_url
  }
  return currentUrl
}

export function realtimeVoiceAfterProviderChange(
  currentVoice: string,
  oldProvider: string,
  newProvider: string,
): string {
  const trimmed = currentVoice.trim()
  if (!trimmed) {
    return realtimeProviderDefaults(newProvider).voice
  }
  if (trimmed === realtimeProviderDefaults(oldProvider).voice.trim()) {
    return realtimeProviderDefaults(newProvider).voice
  }
  return currentVoice
}

function defaultRealtimeConfig(provider: AudioRealtimeProvider = DEFAULT_REALTIME_PROVIDER): AudioRealtimeConfig {
  const defaults = realtimeProviderDefaults(provider)
  return {
    provider,
    ws_url: defaults.ws_url,
    api_key: '',
    api_secret: '',
    app_id: '',
    model: defaults.model,
    voice: defaults.voice,
    instructions: '你是甲壳虫的语音助手。请直接口语化回应，简洁自然，默认使用中文。',
    user_id: '',
    license_key: '',
    device_id: '',
  }
}

export function audioRealtimeConfigured(c: Pick<AudioConfig, 'realtime'> | AudioConfig): boolean {
  const realtime = c.realtime
  if (realtime.provider === 'baidu') {
    return Boolean(
      realtime.app_id.trim() &&
        realtime.api_key.trim() &&
        realtime.api_secret.trim() &&
        realtime.ws_url.trim(),
    )
  }
  return Boolean(
    realtime.api_key.trim() &&
      realtime.model.trim() &&
      realtime.voice.trim() &&
      realtime.ws_url.trim(),
  )
}

export function normalizeAudioConfigForSave(c: AudioConfig): AudioConfig {
  const speech = { ...c.speech }
  const realtime = { ...c.realtime }
  const realtimeDefaults = realtimeProviderDefaults(realtime.provider)

  if (c.service_provider === 'baidu' && !speech.api_url.trim()) {
    speech.api_url = DEFAULT_SPEECH_API_URL_BAIDU
  }
  if (c.service_provider === 'baidu' && !speech.model.trim()) {
    speech.model = '1537'
  }
  if (c.service_provider === 'whisper' && !speech.api_url.trim()) {
    speech.api_url = DEFAULT_SPEECH_API_URL_WHISPER
  }
  if (c.service_provider === 'whisper' && !speech.model.trim()) {
    speech.model = 'whisper-1'
  }

  if (!audioRealtimeProviderSupported(realtime.provider)) {
    realtime.provider = DEFAULT_REALTIME_PROVIDER
  }
  realtime.ws_url = realtime.ws_url.trim()
  if (!realtime.ws_url) {
    realtime.ws_url = realtimeDefaults.ws_url
  }
  if (!realtime.model.trim()) {
    realtime.model = realtimeDefaults.model
  }
  if (!realtime.voice.trim()) {
    realtime.voice = realtimeDefaults.voice
  }

  return { ...c, speech, realtime }
}

export function normalizeAudioConfigFromDevice(raw: Partial<AudioConfig> | null | undefined): AudioConfig {
  const base = defaultAudioConfig()
  if (!raw) return base

  return {
    ...base,
    ...raw,
    microphone: {
      ...base.microphone,
      ...(raw.microphone ?? {}),
      pins: {
        ...base.microphone.pins,
        ...(raw.microphone?.pins ?? {}),
      },
    },
    speaker: {
      ...base.speaker,
      ...(raw.speaker ?? {}),
      pins: {
        ...base.speaker.pins,
        ...(raw.speaker?.pins ?? {}),
      },
    },
    vad: {
      ...base.vad,
      ...(raw.vad ?? {}),
    },
    wake_word: {
      ...base.wake_word,
      ...(raw.wake_word ?? {}),
    },
    speech: {
      ...base.speech,
      ...(raw.speech ?? {}),
    },
    tts: {
      ...base.tts,
      ...(raw.tts ?? {}),
    },
    realtime: {
      ...base.realtime,
      ...(raw.realtime ?? {}),
    },
    ambient_listening: {
      ...base.ambient_listening,
      ...(raw.ambient_listening ?? {}),
      sound_events: Array.isArray(raw.ambient_listening?.sound_events)
        ? [...raw.ambient_listening.sound_events]
        : [...base.ambient_listening.sound_events],
    },
    led_indicator: {
      ...base.led_indicator,
      ...(raw.led_indicator ?? {}),
      states: {
        ...base.led_indicator.states,
        ...(raw.led_indicator?.states ?? {}),
      },
    },
  }
}

export function sampleRateSelectOptions(current: number): number[] {
  const presets: number[] = [...AUDIO_SAMPLE_RATE_PRESETS]
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b)
}

export function bufferSelectOptions(current: number): number[] {
  const presets: number[] = [...AUDIO_BUFFER_PRESETS]
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b)
}

export function ambientIntervalOptions(current: number): number[] {
  const presets: number[] = [...AUDIO_AMBIENT_CHECK_INTERVAL_PRESETS]
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b)
}

export function ambientCooldownOptions(current: number): number[] {
  const presets: number[] = [...AUDIO_AMBIENT_COOLDOWN_PRESETS]
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b)
}

export function unionStringPreset(presets: readonly string[], current: string): string[] {
  if (presets.includes(current)) return [...presets]
  return [current, ...presets]
}

export function unionNumberPreset(presets: readonly number[], current: number): number[] {
  const list: number[] = [...presets]
  if (list.includes(current)) return list
  return [...list, current].sort((a, b) => a - b)
}

export function unionFloatPreset(presets: readonly number[], current: number): number[] {
  const hit = presets.some((preset) => Math.abs(preset - current) < 1e-9)
  if (hit) return [...presets]
  return [...presets, current].sort((a, b) => a - b)
}

export function realtimeRequiredSampleRate(provider: string): number {
  return provider === 'baidu'
    ? AUDIO_REALTIME_BAIDU_SAMPLE_RATE
    : AUDIO_REALTIME_PCM16_SAMPLE_RATE
}

export function defaultAudioConfig(): AudioConfig {
  return {
    version: AUDIO_CONFIG_VERSION,
    enabled: false,
    service_provider: 'baidu',
    microphone: {
      enabled: false,
      device_type: 'i2s_inmp441',
      pins: { ws: 25, sck: 26, din: 27 },
      sample_rate: 16_000,
      bits_per_sample: 16,
      buffer_size: 1024,
    },
    speaker: {
      enabled: false,
      device_type: 'i2s_max98357a',
      pins: { ws: 32, sck: 33, dout: 22, sd: null },
      sample_rate: 16_000,
      bits_per_sample: 16,
    },
    vad: {
      threshold: 0.5,
      silence_duration_ms: 1000,
    },
    wake_word: {
      enabled: false,
      keyword: 'hiesp',
      wake_prompt: '你好，我在听，请说。',
    },
    speech: {
      api_url: DEFAULT_SPEECH_API_URL_BAIDU,
      api_key: '',
      api_secret: '',
      model: '1537',
      language: 'zh',
    },
    tts: {
      voice: '0',
      rate: '+0%',
      pitch: '+0Hz',
    },
    realtime: defaultRealtimeConfig(),
    ambient_listening: {
      enabled: false,
      detect_emotions: true,
      sound_events: ['sigh', 'cough', 'laugh', 'cry', 'door_close'],
      cooldown_minutes: 10,
      check_interval_seconds: 300,
    },
    led_indicator: {
      enabled: false,
      pin: 2,
      states: {
        listening: 'breathing',
        processing: 'fast_blink',
        speaking: 'solid',
      },
    },
  }
}
