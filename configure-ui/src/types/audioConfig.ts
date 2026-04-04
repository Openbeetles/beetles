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

export interface AudioSttConfig {
  provider: string
  api_url: string
  api_key: string
  api_secret: string
  model: string
  language: string
}

export interface AudioTtsConfig {
  provider: string
  voice: string
  rate: string
  pitch: string
}

export interface AudioRealtimeConfig {
  provider: string
  ws_url: string
  api_key: string
  model: string
  voice: string
  instructions: string
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
  microphone: AudioMicrophoneConfig
  speaker: AudioSpeakerConfig
  vad: AudioVadConfig
  wake_word: AudioWakeWordConfig
  stt: AudioSttConfig
  tts: AudioTtsConfig
  realtime: AudioRealtimeConfig
  ambient_listening: AudioAmbientListeningConfig
  led_indicator: AudioLedIndicatorConfig
}

type LegacyAudioRealtimeConfig = Partial<AudioRealtimeConfig> & {
  api_url?: string
}

type LegacyAudioConfig = Partial<AudioConfig> & {
  conversation_mode?: string
  realtime?: LegacyAudioRealtimeConfig | null
}

export const AUDIO_CONFIG_VERSION = 1
export const AUDIO_PIN_MIN = 1
export const AUDIO_PIN_MAX = 48
export const AUDIO_SAMPLE_RATE_MIN = 8_000
export const AUDIO_SAMPLE_RATE_MAX = 48_000
export const AUDIO_REALTIME_PCM16_SAMPLE_RATE = 24_000
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

export const AUDIO_STT_PROVIDERS = ['whisper', 'xunfei', 'baidu'] as const
export const AUDIO_STT_LANGUAGES = ['zh', 'en', 'ja', 'ko'] as const
export const AUDIO_TTS_PROVIDERS = ['edge', 'xunfei', 'baidu'] as const

export const AUDIO_TTS_EDGE_VOICES = [
  'zh-CN-XiaoxiaoNeural',
  'zh-CN-YunxiNeural',
  'en-US-JennyNeural',
  'en-US-GuyNeural',
] as const

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

export const DEFAULT_STT_API_URL_WHISPER = 'https://api.openai.com/v1/audio/transcriptions'
export const DEFAULT_STT_API_URL_BAIDU = 'https://vop.baidu.com/server_api'
export const DEFAULT_REALTIME_WS_URL_OPENAI = 'wss://api.openai.com/v1/realtime'
export const DEFAULT_REALTIME_MODEL_OPENAI = 'gpt-realtime'
export const DEFAULT_REALTIME_VOICE_OPENAI = 'alloy'

function normalizeRealtimeWsUrl(raw: string | null | undefined): string {
  const trimmed = (raw ?? '').trim()
  if (!trimmed) return ''

  const normalized = trimmed.startsWith('https://')
    ? `wss://${trimmed.slice('https://'.length)}`
    : trimmed.startsWith('http://')
      ? `ws://${trimmed.slice('http://'.length)}`
      : trimmed

  const [path, query] = normalized.split('?', 2)
  const basePath = path.replace(/\/+$/, '')
  const nextPath = basePath.endsWith('/realtime') ? basePath : `${basePath}/realtime`
  return query ? `${nextPath}?${query}` : nextPath
}

function defaultRealtimeConfig(): AudioRealtimeConfig {
  return {
    provider: 'openai_compatible',
    ws_url: DEFAULT_REALTIME_WS_URL_OPENAI,
    api_key: '',
    model: DEFAULT_REALTIME_MODEL_OPENAI,
    voice: DEFAULT_REALTIME_VOICE_OPENAI,
    instructions: '你是甲壳虫的语音助手。请直接口语化回应，简洁自然，默认使用中文。',
  }
}

export function audioRealtimeConfigured(c: Pick<AudioConfig, 'realtime'> | AudioConfig): boolean {
  const realtime = c.realtime
  return Boolean(
    realtime.api_key.trim() &&
      realtime.model.trim() &&
      realtime.voice.trim() &&
      realtime.ws_url.trim(),
  )
}

export function normalizeAudioConfigForSave(c: AudioConfig): AudioConfig {
  const stt = { ...c.stt }
  const realtime = { ...c.realtime }

  if (stt.provider === 'baidu' && !stt.api_url.trim()) {
    stt.api_url = DEFAULT_STT_API_URL_BAIDU
  }
  if (stt.provider === 'baidu' && !stt.model.trim()) {
    stt.model = '1537'
  }
  if (stt.provider === 'whisper' && !stt.api_url.trim()) {
    stt.api_url = DEFAULT_STT_API_URL_WHISPER
  }
  if (stt.provider === 'whisper' && !stt.model.trim()) {
    stt.model = 'whisper-1'
  }

  realtime.provider = 'openai_compatible'
  realtime.ws_url = realtime.ws_url.trim()
    ? normalizeRealtimeWsUrl(realtime.ws_url)
    : realtime.ws_url.trim()
  if (!realtime.model.trim()) {
    realtime.model = DEFAULT_REALTIME_MODEL_OPENAI
  }
  if (!realtime.voice.trim()) {
    realtime.voice = DEFAULT_REALTIME_VOICE_OPENAI
  }

  return { ...c, stt, realtime }
}

export function normalizeAudioConfigFromDevice(raw: LegacyAudioConfig | null | undefined): AudioConfig {
  const base = defaultAudioConfig()
  if (!raw) return base

  const rawRealtime = (raw.realtime ?? {}) as LegacyAudioRealtimeConfig
  const migratedWsUrl =
    typeof rawRealtime.ws_url === 'string'
      ? normalizeRealtimeWsUrl(rawRealtime.ws_url)
      : typeof rawRealtime.api_url === 'string' && rawRealtime.api_url.trim()
        ? normalizeRealtimeWsUrl(rawRealtime.api_url)
        : base.realtime.ws_url

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
    stt: {
      ...base.stt,
      ...(raw.stt ?? {}),
    },
    tts: {
      ...base.tts,
      ...(raw.tts ?? {}),
    },
    realtime: {
      ...base.realtime,
      ...rawRealtime,
      ws_url: migratedWsUrl,
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

export function defaultAudioConfig(): AudioConfig {
  return {
    version: AUDIO_CONFIG_VERSION,
    enabled: false,
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
    stt: {
      provider: 'baidu',
      api_url: DEFAULT_STT_API_URL_BAIDU,
      api_key: '',
      api_secret: '',
      model: '1537',
      language: 'zh',
    },
    tts: {
      provider: 'baidu',
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
