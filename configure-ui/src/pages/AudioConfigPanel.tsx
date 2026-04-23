import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Checkbox from '@mui/material/Checkbox'
import FormControl from '@mui/material/FormControl'
import FormControlLabel from '@mui/material/FormControlLabel'
import FormGroup from '@mui/material/FormGroup'
import InputLabel from '@mui/material/InputLabel'
import MenuItem from '@mui/material/MenuItem'
import Select from '@mui/material/Select'
import Switch from '@mui/material/Switch'
import Tab from '@mui/material/Tab'
import Tabs from '@mui/material/Tabs'
import TextField from '@mui/material/TextField'
import Typography from '@mui/material/Typography'
import SaveRounded from '@mui/icons-material/SaveRounded'
import type { SelectChangeEvent } from '@mui/material/Select'
import {
  FormFieldStack,
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSub,
  FormSectionSubCollapsible,
  InlineAlert,
  PageLoadErrorState,
  SaveFeedback,
  splitPageErrorState,
} from '../components/form'
import { Os3dIcon } from '../components/Os3dIcon'
import { SettingsSection } from '../components/SettingsSection'
import { OS_ICON_DEVICE_CONFIG } from '../config/osIcons'
import { PAGE_COLUMN_FILL_SX, PAGE_STACK_OUTER_SX } from '../theme/panelStyles'
import { useConfig } from '../hooks/useConfig'
import { useDeviceApi } from '../hooks/useDeviceApi'
import { translateApiError } from '../i18n/apiErrors'
import { useRevealedPasswordFields } from '../hooks/useRevealedPassword'
import { useConfigEditorController } from '../hooks/useConfigEditorController'
import { useDeviceRuntimeKind } from '../store/deviceStatusStore'
import type { HardwareDiscoveryItem } from '../api/endpoints/hardware'
import {
  AUDIO_AMBIENT_SOUND_EVENT_PRESETS,
  AUDIO_BITS_PER_SAMPLE_ALLOWED,
  AUDIO_LED_STATE_PRESETS,
  AUDIO_MIC_DEVICE_TYPES,
  AUDIO_REALTIME_PROVIDERS,
  AUDIO_SPEECH_LANGUAGES,
  AUDIO_SPEECH_PROVIDERS,
  AUDIO_SPEAKER_DEVICE_TYPES,
  AUDIO_SPEAKER_DEVICE_TYPES_LINUX,
  AUDIO_TTS_PITCH_PRESETS,
  AUDIO_TTS_RATE_PRESETS,
  AUDIO_VAD_SILENCE_MS_PRESETS,
  AUDIO_VAD_THRESHOLD_PRESETS,
  ambientCooldownOptions,
  ambientIntervalOptions,
  audioSpeakerPinsOrDefault,
  bufferSelectOptions,
  DEFAULT_SPEECH_API_URL_BAIDU,
  DEFAULT_SPEECH_API_URL_WHISPER,
  audioRealtimeConfigured,
  defaultAudioConfig,
  normalizeAudioConfigFromDevice,
  normalizeAudioConfigForSave,
  realtimeModelAfterProviderChange,
  realtimeRequiredSampleRate,
  realtimeVoiceAfterProviderChange,
  realtimeWsUrlAfterProviderChange,
  sampleRateSelectOptions,
  unionFloatPreset,
  unionNumberPreset,
  unionStringPreset,
  type AudioConfig,
} from '../types/audioConfig'
import { validateAudioConfig } from './audioConfigValidation'

function asNumber(v: string): number | null {
  const n = Number(v)
  return Number.isFinite(n) ? n : null
}

type PresetSound = (typeof AUDIO_AMBIENT_SOUND_EVENT_PRESETS)[number]

function presetSoundEventsSelected(events: string[]): Set<string> {
  const s = new Set<string>()
  for (const e of events) {
    if ((AUDIO_AMBIENT_SOUND_EVENT_PRESETS as readonly string[]).includes(e)) {
      s.add(e)
    }
  }
  return s
}

function extraSoundEvents(events: string[]): string[] {
  return events.filter(
    (e) => !(AUDIO_AMBIENT_SOUND_EVENT_PRESETS as readonly string[]).includes(e),
  )
}

function usbSpeakerSupportsInput(item: HardwareDiscoveryItem): boolean {
  return item.capabilities.includes('audio_input')
}

export function AudioConfigPanel() {
  const { t } = useTranslation()
  const runtimeKind = useDeviceRuntimeKind()
  const { api } = useDeviceApi()
  const {
    audioConfig,
    audioLoading,
    audioError,
    loadAudioConfig,
    saveAudioConfig,
  } = useConfig()
  const editor = useConfigEditorController({
    t,
    hasData: audioConfig !== null,
    loading: audioLoading,
    load: loadAudioConfig,
  })
  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields()
  const [draft, setDraft] = useState<AudioConfig | null>(null)
  const [audioTab, setAudioTab] = useState(0)
  const [saveRestartRequired, setSaveRestartRequired] = useState(false)
  const [usbAudioDevices, setUsbAudioDevices] = useState<HardwareDiscoveryItem[]>([])
  const [usbAudioLoading, setUsbAudioLoading] = useState(false)
  const [usbAudioError, setUsbAudioError] = useState<string | null>(null)
  const linuxRuntime = runtimeKind === 'linux'
  const formSource = draft ?? audioConfig
  const rawForm = normalizeAudioConfigFromDevice(formSource ?? defaultAudioConfig())
  const form: AudioConfig = linuxRuntime
    ? {
        ...rawForm,
        microphone: {
          ...rawForm.microphone,
          enabled: false,
        },
        speaker: {
          ...rawForm.speaker,
          device_type: 'usb',
          pins: null,
        },
      }
    : rawForm
  const loadErrorState = splitPageErrorState({
    hasData: Boolean(formSource),
    loading: audioLoading,
    error: audioError,
  })
  const speakerPins = audioSpeakerPinsOrDefault(form.speaker.pins)

  const saveDisabled = editor.saveDisabled
  const activeAudioTab = form.enabled ? audioTab : 0
  const setDraftSafe = (next: AudioConfig) => {
    editor.markDirty()
    setDraft(next)
  }

  const save = async () => {
    await editor.runSave({
      validate: () => validateAudioConfig(form, runtimeKind, t),
      onBeforeSave: () => setSaveRestartRequired(false),
      performSave: () => saveAudioConfig(normalizeAudioConfigForSave(form)),
      onSuccess: (result) => {
        setSaveRestartRequired(Boolean(result.restartRequired))
      },
    })
  }

  const fieldGridSx = {
    display: 'grid',
    gap: 2,
    gridTemplateColumns: {
      xs: 'minmax(0, 1fr)',
      md: 'repeat(2, minmax(0, 1fr))',
    },
    '& .MuiFormControl-root': {
      minWidth: 0,
    },
  } as const
  const audioOn = form.enabled
  const realtimeReady = audioRealtimeConfigured(form)
  const realtimeWakeEnabled = realtimeReady && form.wake_word.enabled
  const realtimeSampleRate = realtimeRequiredSampleRate(form.realtime.provider)
  const micOn = audioOn && form.microphone.enabled
  const spkOn = audioOn && form.speaker.enabled
  const showAcousticWake = micOn
  const showSpeechInput = micOn
  const showSpeechOutput = spkOn
  const showSpeechCredentials = showSpeechInput || showSpeechOutput
  const showRealtime = audioOn && form.wake_word.enabled
  const showAmbientBlock = micOn
  const showLedBlock = audioOn
  const showWakePrompt = form.wake_word.enabled
  const speakerDeviceTypes = linuxRuntime
    ? [...AUDIO_SPEAKER_DEVICE_TYPES_LINUX]
    : [...AUDIO_SPEAKER_DEVICE_TYPES]
  const speakerUsesUsbDevice = linuxRuntime && form.speaker.device_type === 'usb'
  const selectedUsbAudioDevice =
    usbAudioDevices.find((item) => item.device_ref === (form.speaker.device_ref ?? '')) ?? null
  const usbAudioEmpty = !usbAudioLoading && usbAudioDevices.length === 0
  const usbAudioFieldError = usbAudioError ?? (usbAudioEmpty ? t('audioConfig.speakerUsbScanEmpty') : null)

  const refreshUsbAudioDevices = async () => {
    if (!linuxRuntime || !audioOn || !spkOn || form.speaker.device_type !== 'usb') {
      setUsbAudioDevices([])
      setUsbAudioError(null)
      setUsbAudioLoading(false)
      return
    }
    setUsbAudioLoading(true)
    const res = await api.hardware.discover('usb', 'audio_output')
    if (res.ok && res.data) {
      setUsbAudioDevices(Array.isArray(res.data.items) ? res.data.items : [])
      setUsbAudioError(null)
    } else {
      setUsbAudioDevices([])
      setUsbAudioError(translateApiError(t, res.error, 'audioConfig.speakerUsbScanFailed'))
    }
    setUsbAudioLoading(false)
  }

  useEffect(() => {
    queueMicrotask(() => {
      void refreshUsbAudioDevices()
    })
    // refreshUsbAudioDevices 为组件内异步闭包，纳入 deps 会导致每轮渲染重跑
    // eslint-disable-next-line react-hooks/exhaustive-deps -- USB 列表随音频开关/设备类型刷新
  }, [audioOn, api.hardware, form.speaker.device_type, linuxRuntime, spkOn])

  if (audioLoading && !formSource) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.audio} />}
          label={t('audioConfig.sectionMain')}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    )
  }

  if (!formSource) {
    return (
      <Box sx={PAGE_STACK_OUTER_SX}>
        <SettingsSection
          pinHeader
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.audio} />}
          label={t('audioConfig.sectionMain')}
          description={t('audioConfig.sectionMainDesc')}
        >
          {loadErrorState.blockingError ? (
            <PageLoadErrorState
              message={loadErrorState.blockingError}
              onRetry={loadAudioConfig}
            />
          ) : (
            <PanelStateBlock
              tone="neutral"
              icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.audio} variant="inline" />}
              title={t('config.unavailableTitle')}
              description={t('config.unavailableDesc')}
            />
          )}
        </SettingsSection>
      </Box>
    )
  }

  const togglePresetSoundEvent = (ev: PresetSound) => {
    const extras = extraSoundEvents(form.ambient_listening.sound_events)
    const presets = AUDIO_AMBIENT_SOUND_EVENT_PRESETS.filter((p) =>
      form.ambient_listening.sound_events.includes(p),
    )
    const has = presets.includes(ev)
    const nextPresets = has ? presets.filter((p) => p !== ev) : [...presets, ev]
    const merged = [...nextPresets, ...extras].slice(0, 16)
    setDraftSafe({
      ...form,
      ambient_listening: { ...form.ambient_listening, sound_events: merged },
    })
  }

  const setExtraSoundEventsStr = (raw: string) => {
    const parsed = raw
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
      .slice(0, 16)
    const presets = AUDIO_AMBIENT_SOUND_EVENT_PRESETS.filter((p) =>
      form.ambient_listening.sound_events.includes(p),
    )
    const merged = [...new Set([...presets, ...parsed])].slice(0, 16)
    setDraftSafe({
      ...form,
      ambient_listening: { ...form.ambient_listening, sound_events: merged },
    })
  }

  const extrasStr = extraSoundEvents(form.ambient_listening.sound_events).join(', ')
  const speechRoutingDescription = realtimeWakeEnabled
    ? t('audioConfig.realtimeActiveHelp')
    : t('audioConfig.speechFallbackHelp')

  const setSpeakerSampleRate = (sampleRate: number) => {
    setDraftSafe({
      ...form,
      speaker: {
        ...form.speaker,
        sample_rate: sampleRate,
      },
    })
  }

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={loadErrorState.inlineError} onRetry={loadAudioConfig} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.audio} />}
        label={t('audioConfig.sectionMain')}
        description={t('audioConfig.sectionMainDesc')}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={save}
            disabled={saveDisabled}
          >
            {editor.saveFeedback.status === 'saving' ? t('common.saving') : t('common.save')}
          </Button>
        }
        belowTitleRow={
          editor.saveFeedback.status === 'ok' || editor.saveFeedback.status === 'fail' ? (
            <SaveFeedback
              placement="belowTitle"
              status={editor.saveFeedback.status}
              message={
                editor.saveFeedback.status === 'ok'
                  ? saveRestartRequired
                    ? t('audioConfig.restartRequired')
                    : t('common.saveOk')
                  : editor.saveFeedback.error
              }
              autoDismissMs={3000}
              onDismiss={editor.saveFeedback.dismiss}
            />
          ) : null
        }
      >
        <FormFieldStack>
          <FormSectionSub title={t('audioConfig.sectionBasic')}>
            <FormControlLabel
              control={
                <Switch
                  checked={form.enabled}
                  onChange={(_, checked) => setDraftSafe({ ...form, enabled: checked })}
                />
              }
              label={t('audioConfig.enabled')}
            />
            {!audioOn ? (
              <Typography variant="body2" sx={{ mt: 1, color: "var(--text-tertiary)" }}>
                {t('audioConfig.hintEnableAudioFirst')}
              </Typography>
            ) : null}
          </FormSectionSub>

          {audioOn ? (
            <>
              <Tabs
                value={activeAudioTab}
                onChange={(_, v) => setAudioTab(v)}
                variant="scrollable"
                allowScrollButtonsMobile
                sx={{
                  borderBottom: 'none',
                  
                  minHeight: 44,
                  '& .MuiTab-root': {
                    minHeight: 44,
                    fontSize: 'var(--font-size-body-sm)',
                  },
                }}
              >
                <Tab label={t('audioConfig.tabDevices')} />
                <Tab label={t('audioConfig.tabSpeech')} />
                <Tab label={t('audioConfig.tabMore')} />
              </Tabs>
              <Box sx={{ pt: 2.5 }}>
                {activeAudioTab === 0 && (
                  <>
              <FormSectionSub title={t('audioConfig.sectionMicrophone')}>
                <FormControlLabel
                  control={
                    <Switch
                      checked={form.microphone.enabled}
                      onChange={(_, checked) =>
                        setDraftSafe({
                          ...form,
                          microphone: { ...form.microphone, enabled: checked },
                        })
                      }
                    />
                  }
                  label={t('audioConfig.microphoneEnabled')}
                />
                {micOn ? (
                  <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                    <Box sx={fieldGridSx}>
                    <FormControl fullWidth>
                      <InputLabel id="mic-device-type">{t('audioConfig.deviceType')}</InputLabel>
                      <Select
                        labelId="mic-device-type"
                        label={t('audioConfig.deviceType')}
                        value={form.microphone.device_type}
                        onChange={(e: SelectChangeEvent) =>
                          setDraftSafe({
                            ...form,
                            microphone: { ...form.microphone, device_type: e.target.value },
                          })
                        }
                      >
                        {unionStringPreset(
                          [...AUDIO_MIC_DEVICE_TYPES],
                          form.microphone.device_type,
                        ).map((dt) => (
                          <MenuItem key={dt} value={dt}>
                            {(AUDIO_MIC_DEVICE_TYPES as readonly string[]).includes(dt)
                              ? t(`audioConfig.deviceMic.${dt}`)
                              : dt}
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    <FormControl fullWidth>
                      <InputLabel id="mic-sr">{t('audioConfig.sampleRate')}</InputLabel>
                      <Select
                        labelId="mic-sr"
                        label={t('audioConfig.sampleRate')}
                        value={String(form.microphone.sample_rate)}
                        onChange={(e: SelectChangeEvent) => {
                          const v = asNumber(e.target.value)
                          if (v == null) return
                          setDraftSafe({
                            ...form,
                            microphone: { ...form.microphone, sample_rate: Math.trunc(v) },
                          })
                        }}
                      >
                        {sampleRateSelectOptions(form.microphone.sample_rate).map((sr) => (
                          <MenuItem key={sr} value={String(sr)}>
                            {sr} Hz
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    </Box>
                    <Box sx={fieldGridSx}>
                    <FormControl fullWidth>
                      <InputLabel id="mic-bits">{t('audioConfig.bitsPerSample')}</InputLabel>
                      <Select
                        labelId="mic-bits"
                        label={t('audioConfig.bitsPerSample')}
                        value={String(form.microphone.bits_per_sample)}
                        onChange={(e: SelectChangeEvent) => {
                          const v = asNumber(e.target.value)
                          if (v == null) return
                          setDraftSafe({
                            ...form,
                            microphone: { ...form.microphone, bits_per_sample: Math.trunc(v) },
                          })
                        }}
                      >
                        {AUDIO_BITS_PER_SAMPLE_ALLOWED.map((b) => (
                          <MenuItem key={b} value={String(b)}>
                            {b}
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    <FormControl fullWidth>
                      <InputLabel id="mic-buf">{t('audioConfig.bufferSize')}</InputLabel>
                      <Select
                        labelId="mic-buf"
                        label={t('audioConfig.bufferSize')}
                        value={String(form.microphone.buffer_size)}
                        onChange={(e: SelectChangeEvent) => {
                          const v = asNumber(e.target.value)
                          if (v == null) return
                          setDraftSafe({
                            ...form,
                            microphone: { ...form.microphone, buffer_size: Math.trunc(v) },
                          })
                        }}
                      >
                        {bufferSelectOptions(form.microphone.buffer_size).map((n) => (
                          <MenuItem key={n} value={String(n)}>
                            {n} B
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    <FormControl fullWidth>
                      <InputLabel id="vad-th">{t('audioConfig.vadThreshold')}</InputLabel>
                      <Select
                        labelId="vad-th"
                        label={t('audioConfig.vadThreshold')}
                        value={String(form.vad.threshold)}
                        onChange={(e: SelectChangeEvent) => {
                          const v = Number(e.target.value)
                          if (!Number.isFinite(v)) return
                          setDraftSafe({ ...form, vad: { ...form.vad, threshold: v } })
                        }}
                      >
                        {unionFloatPreset(
                          [...AUDIO_VAD_THRESHOLD_PRESETS],
                          form.vad.threshold,
                        ).map((th) => (
                          <MenuItem key={th} value={String(th)}>
                            {th}
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    <FormControl fullWidth>
                      <InputLabel id="vad-sil">{t('audioConfig.vadSilenceMs')}</InputLabel>
                      <Select
                        labelId="vad-sil"
                        label={t('audioConfig.vadSilenceMs')}
                        value={String(form.vad.silence_duration_ms)}
                        onChange={(e: SelectChangeEvent) => {
                          const v = asNumber(e.target.value)
                          if (v == null) return
                          setDraftSafe({
                            ...form,
                            vad: {
                              ...form.vad,
                              silence_duration_ms: Math.trunc(v),
                            },
                          })
                        }}
                      >
                        {unionNumberPreset(
                          [...AUDIO_VAD_SILENCE_MS_PRESETS],
                          form.vad.silence_duration_ms,
                        ).map((ms) => (
                          <MenuItem key={ms} value={String(ms)}>
                            {ms} ms
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                    <TextField
                      label={t('audioConfig.pinWs')}
                      value={String(form.microphone.pins.ws)}
                      onChange={(e) => {
                        const v = asNumber(e.target.value)
                        if (v == null) return
                        setDraftSafe({
                          ...form,
                          microphone: {
                            ...form.microphone,
                            pins: { ...form.microphone.pins, ws: Math.trunc(v) },
                          },
                        })
                      }}
                    />
                    <TextField
                      label={t('audioConfig.pinSck')}
                      value={String(form.microphone.pins.sck)}
                      onChange={(e) => {
                        const v = asNumber(e.target.value)
                        if (v == null) return
                        setDraftSafe({
                          ...form,
                          microphone: {
                            ...form.microphone,
                            pins: { ...form.microphone.pins, sck: Math.trunc(v) },
                          },
                        })
                      }}
                    />
                    <TextField
                      label={t('audioConfig.pinDin')}
                      value={String(form.microphone.pins.din)}
                      onChange={(e) => {
                        const v = asNumber(e.target.value)
                        if (v == null) return
                        setDraftSafe({
                          ...form,
                          microphone: {
                            ...form.microphone,
                            pins: { ...form.microphone.pins, din: Math.trunc(v) },
                          },
                        })
                      }}
                    />
                    </Box>
                  </Box>
                ) : null}
              </FormSectionSub>
              <FormSectionSub title={t('audioConfig.sectionSpeaker')}>
                <FormControlLabel
                  control={
                    <Switch
                      checked={form.speaker.enabled}
                      onChange={(_, checked) =>
                        setDraftSafe({
                          ...form,
                          speaker: { ...form.speaker, enabled: checked },
                        })
                      }
                    />
                  }
                  label={t('audioConfig.speakerEnabled')}
                />
                {spkOn ? (
                  <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                    <Box sx={fieldGridSx}>
                      <FormControl fullWidth>
                        <InputLabel id="spk-device-type">{t('audioConfig.deviceType')}</InputLabel>
                        <Select
                          labelId="spk-device-type"
                          label={t('audioConfig.deviceType')}
                          value={form.speaker.device_type}
                          onChange={(e: SelectChangeEvent) =>
                            setDraftSafe({
                              ...form,
                              speaker: { ...form.speaker, device_type: e.target.value },
                            })
                          }
                        >
                          {unionStringPreset(
                            speakerDeviceTypes,
                            form.speaker.device_type,
                          ).map((dt) => (
                            <MenuItem key={dt} value={dt}>
                              {([...AUDIO_SPEAKER_DEVICE_TYPES, ...AUDIO_SPEAKER_DEVICE_TYPES_LINUX] as readonly string[]).includes(dt)
                                ? t(`audioConfig.deviceSpeaker.${dt}`)
                                : dt}
                            </MenuItem>
                          ))}
                        </Select>
                      </FormControl>
                      <FormControl fullWidth>
                        <InputLabel id="spk-sr">{t('audioConfig.sampleRate')}</InputLabel>
                        <Select
                          labelId="spk-sr"
                          label={t('audioConfig.sampleRate')}
                          value={String(form.speaker.sample_rate)}
                          onChange={(e: SelectChangeEvent) => {
                            const v = asNumber(e.target.value)
                            if (v == null) return
                            setSpeakerSampleRate(Math.trunc(v))
                          }}
                        >
                          {sampleRateSelectOptions(form.speaker.sample_rate).map((sr) => (
                            <MenuItem key={sr} value={String(sr)}>
                              {sr} Hz
                            </MenuItem>
                          ))}
                        </Select>
                      </FormControl>
                      {speakerUsesUsbDevice ? (
                        <Box
                          sx={{
                            display: 'flex',
                            gap: 1.5,
                            alignItems: 'stretch',
                            flexWrap: {
                              xs: 'wrap',
                              md: 'nowrap',
                            },
                            gridColumn: {
                              xs: 'span 1',
                              md: 'span 1',
                            },
                          }}
                        >
                          <FormControl
                            fullWidth
                            sx={{
                              flex: '1 1 auto',
                            }}
                          >
                            <InputLabel id="spk-device-ref">{t('audioConfig.speakerUsbDevice')}</InputLabel>
                            <Select
                              labelId="spk-device-ref"
                              label={t('audioConfig.speakerUsbDevice')}
                              value={form.speaker.device_ref ?? ''}
                              error={Boolean(usbAudioFieldError)}
                              onChange={(e: SelectChangeEvent) =>
                                setDraftSafe({
                                  ...form,
                                  speaker: { ...form.speaker, device_ref: e.target.value || null },
                                })
                              }
                            >
                              {usbAudioDevices.map((item) => (
                                <MenuItem key={item.device_ref} value={item.device_ref}>
                                  {usbSpeakerSupportsInput(item)
                                    ? `${item.label} · ${t('audioConfig.speakerUsbComboSuffix')}`
                                    : item.label}
                                </MenuItem>
                              ))}
                            </Select>
                            {usbAudioFieldError ? (
                              <Typography variant="body2" color="error.main" sx={{ mt: 0.75 }}>
                                {usbAudioFieldError}
                              </Typography>
                            ) : null}
                          </FormControl>
                          <Button
                            size="small"
                            variant="outlined"
                            onClick={() => {
                              void refreshUsbAudioDevices()
                            }}
                            disabled={usbAudioLoading}
                            sx={{
                              flexShrink: 0,
                              minWidth: 96,
                              height: 40,
                              alignSelf: {
                                xs: 'stretch',
                                md: 'flex-start',
                              },
                            }}
                          >
                            {t('audioConfig.speakerUsbScanAction')}
                          </Button>
                        </Box>
                      ) : null}
                    </Box>
                    <Box sx={fieldGridSx}>
                      <FormControl fullWidth>
                        <InputLabel id="spk-bits">{t('audioConfig.bitsPerSample')}</InputLabel>
                        <Select
                          labelId="spk-bits"
                          label={t('audioConfig.bitsPerSample')}
                          value={String(form.speaker.bits_per_sample)}
                          onChange={(e: SelectChangeEvent) => {
                            const v = asNumber(e.target.value)
                            if (v == null) return
                            setDraftSafe({
                              ...form,
                              speaker: { ...form.speaker, bits_per_sample: Math.trunc(v) },
                            })
                          }}
                        >
                          {AUDIO_BITS_PER_SAMPLE_ALLOWED.map((b) => (
                            <MenuItem key={b} value={String(b)}>
                              {b}
                            </MenuItem>
                          ))}
                        </Select>
                      </FormControl>
                      {!speakerUsesUsbDevice ? (
                        <>
                          <TextField
                            label={t('audioConfig.pinWs')}
                            value={String(speakerPins.ws)}
                            onChange={(e) => {
                              const v = asNumber(e.target.value)
                              if (v == null) return
                              setDraftSafe({
                                ...form,
                                speaker: {
                                  ...form.speaker,
                                  pins: { ...speakerPins, ws: Math.trunc(v) },
                                },
                              })
                            }}
                          />
                          <TextField
                            label={t('audioConfig.pinSck')}
                            value={String(speakerPins.sck)}
                            onChange={(e) => {
                              const v = asNumber(e.target.value)
                              if (v == null) return
                              setDraftSafe({
                                ...form,
                                speaker: {
                                  ...form.speaker,
                                  pins: { ...speakerPins, sck: Math.trunc(v) },
                                },
                              })
                            }}
                          />
                          <TextField
                            label={t('audioConfig.pinDout')}
                            value={String(speakerPins.dout)}
                            onChange={(e) => {
                              const v = asNumber(e.target.value)
                              if (v == null) return
                              setDraftSafe({
                                ...form,
                                speaker: {
                                  ...form.speaker,
                                  pins: { ...speakerPins, dout: Math.trunc(v) },
                                },
                              })
                            }}
                          />
                          <TextField
                            label={t('audioConfig.pinSdOptional')}
                            placeholder={t('audioConfig.pinOptionalPlaceholder')}
                            value={speakerPins.sd != null ? String(speakerPins.sd) : ''}
                            onChange={(e) => {
                              const raw = e.target.value.trim()
                              if (raw === '') {
                                setDraftSafe({
                                  ...form,
                                  speaker: {
                                    ...form.speaker,
                                    pins: { ...speakerPins, sd: null },
                                  },
                                })
                                return
                              }
                              const v = asNumber(raw)
                              if (v == null) return
                              setDraftSafe({
                                ...form,
                                speaker: {
                                  ...form.speaker,
                                  pins: { ...speakerPins, sd: Math.trunc(v) },
                                },
                              })
                            }}
                          />
                        </>
                      ) : null}
                    </Box>
                    {speakerUsesUsbDevice && selectedUsbAudioDevice && usbSpeakerSupportsInput(selectedUsbAudioDevice) ? (
                      <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
                        {t('audioConfig.speakerUsbComboHint')}
                      </Typography>
                    ) : null}
                  </Box>
                ) : null}
              </FormSectionSub>
                  </>
                )}

                {activeAudioTab === 1 && (
                  <>
                    <FormSectionSub title={t('audioConfig.sectionSpeechRouting')}>
                      <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
                        {speechRoutingDescription}
                      </Typography>
                    </FormSectionSub>

                    {showSpeechInput || showSpeechOutput ? (
                      <FormSectionSubCollapsible title={t('audioConfig.sectionSpeechService')}>
                        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                          {realtimeWakeEnabled ? (
                            <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
                              {t('audioConfig.speechFallbackReservedHelp')}
                            </Typography>
                          ) : null}
                          <Box sx={fieldGridSx}>
                            <FormControl fullWidth>
                              <InputLabel id="speech-provider">{t('audioConfig.serviceProvider')}</InputLabel>
                              <Select
                                labelId="speech-provider"
                                label={t('audioConfig.serviceProvider')}
                                value={form.service_provider}
                                onChange={(e: SelectChangeEvent) => {
                                  const provider = e.target.value
                                  setDraftSafe({
                                    ...form,
                                    service_provider: provider,
                                    speech: {
                                      ...form.speech,
                                      api_url:
                                        provider === 'whisper'
                                          ? DEFAULT_SPEECH_API_URL_WHISPER
                                          : provider === 'baidu'
                                            ? DEFAULT_SPEECH_API_URL_BAIDU
                                            : form.speech.api_url,
                                      model:
                                        provider === 'whisper'
                                          ? 'whisper-1'
                                          : provider === 'baidu'
                                            ? '1537'
                                            : form.speech.model,
                                    },
                                  })
                                }}
                              >
                                {unionStringPreset([...AUDIO_SPEECH_PROVIDERS], form.service_provider).map(
                                  (provider) => (
                                    <MenuItem key={provider} value={provider}>
                                      {(AUDIO_SPEECH_PROVIDERS as readonly string[]).includes(provider)
                                        ? t(`audioConfig.serviceProviderLabels.${provider}`)
                                        : provider}
                                    </MenuItem>
                                  ),
                                )}
                              </Select>
                            </FormControl>
                            {showSpeechCredentials ? (
                              <TextField
                                label={t('audioConfig.speechApiKey')}
                                type={isRevealed('audio_speech_api_key') ? 'text' : 'password'}
                                value={form.speech.api_key}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    speech: { ...form.speech, api_key: e.target.value },
                                  })
                                }
                                slotProps={{
                                  htmlInput: {
                                    style: { fontFamily: 'var(--font-mono)' },
                                    ...getRevealHandlers('audio_speech_api_key'),
                                  },
                                }}
                              />
                            ) : null}
                            {showSpeechCredentials ? (
                              <TextField
                                label={t('audioConfig.speechApiSecret')}
                                type={isRevealed('audio_speech_api_secret') ? 'text' : 'password'}
                                value={form.speech.api_secret}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    speech: { ...form.speech, api_secret: e.target.value },
                                  })
                                }
                                slotProps={{
                                  htmlInput: {
                                    style: { fontFamily: 'var(--font-mono)' },
                                    ...getRevealHandlers('audio_speech_api_secret'),
                                  },
                                }}
                              />
                            ) : null}
                            {showSpeechOutput ? (
                              <TextField
                                label={t('audioConfig.ttsVoice')}
                                value={form.tts.voice}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    tts: { ...form.tts, voice: e.target.value.trim() },
                                  })
                                }
                              />
                            ) : null}
                          </Box>
                          <Box sx={fieldGridSx}>
                            {showSpeechInput ? (
                              <FormControl fullWidth>
                                <InputLabel id="speech-lang">{t('audioConfig.speechLanguage')}</InputLabel>
                                <Select
                                  labelId="speech-lang"
                                  label={t('audioConfig.speechLanguage')}
                                  value={form.speech.language}
                                  onChange={(e: SelectChangeEvent) =>
                                    setDraftSafe({
                                      ...form,
                                      speech: { ...form.speech, language: e.target.value },
                                    })
                                  }
                                >
                                  {unionStringPreset(
                                    [...AUDIO_SPEECH_LANGUAGES],
                                    form.speech.language,
                                  ).map((lang) => (
                                    <MenuItem key={lang} value={lang}>
                                      {(AUDIO_SPEECH_LANGUAGES as readonly string[]).includes(lang)
                                        ? t(`audioConfig.speechLangLabels.${lang}`)
                                        : lang}
                                    </MenuItem>
                                  ))}
                                </Select>
                              </FormControl>
                            ) : null}
                            {showSpeechInput ? (
                              <TextField
                                label={t('audioConfig.speechModel')}
                                value={form.speech.model}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    speech: {
                                      ...form.speech,
                                      model: e.target.value.trim(),
                                    },
                                  })
                                }
                              />
                            ) : null}
                            {showSpeechInput ? (
                              <TextField
                                sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                                label={t('audioConfig.speechApiUrl')}
                                value={form.speech.api_url}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    speech: {
                                      ...form.speech,
                                      api_url: e.target.value.trim(),
                                    },
                                  })
                                }
                              />
                            ) : null}
                            {showSpeechOutput ? (
                              <FormControl fullWidth>
                                <InputLabel id="tts-rate">{t('audioConfig.ttsRate')}</InputLabel>
                                <Select
                                  labelId="tts-rate"
                                  label={t('audioConfig.ttsRate')}
                                  value={form.tts.rate}
                                  onChange={(e: SelectChangeEvent) =>
                                    setDraftSafe({
                                      ...form,
                                      tts: { ...form.tts, rate: e.target.value },
                                    })
                                  }
                                >
                                  {unionStringPreset(
                                    [...AUDIO_TTS_RATE_PRESETS],
                                    form.tts.rate,
                                  ).map((rate) => (
                                    <MenuItem key={rate} value={rate}>
                                      {rate}
                                    </MenuItem>
                                  ))}
                                </Select>
                              </FormControl>
                            ) : null}
                            {showSpeechOutput ? (
                              <FormControl fullWidth>
                                <InputLabel id="tts-pitch">{t('audioConfig.ttsPitch')}</InputLabel>
                                <Select
                                  labelId="tts-pitch"
                                  label={t('audioConfig.ttsPitch')}
                                  value={form.tts.pitch}
                                  onChange={(e: SelectChangeEvent) =>
                                    setDraftSafe({
                                      ...form,
                                      tts: { ...form.tts, pitch: e.target.value },
                                    })
                                  }
                                >
                                  {unionStringPreset(
                                    [...AUDIO_TTS_PITCH_PRESETS],
                                    form.tts.pitch,
                                  ).map((pitch) => (
                                    <MenuItem key={pitch} value={pitch}>
                                      {pitch}
                                    </MenuItem>
                                  ))}
                                </Select>
                              </FormControl>
                            ) : null}
                          </Box>
                        </Box>
                      </FormSectionSubCollapsible>
                    ) : null}

                    {showAcousticWake ? (
                      <Box sx={{ mt: 3, display: 'flex', flexDirection: 'column', gap: 2 }}>
                        <FormControlLabel
                          control={
                            <Switch
                              checked={form.wake_word.enabled}
                              onChange={(_, checked) =>
                                setDraftSafe({
                                  ...form,
                                  wake_word: { ...form.wake_word, enabled: checked },
                                })
                              }
                            />
                          }
                          label={t('audioConfig.wakeWordEnabled')}
                        />
                        {form.wake_word.enabled ? (
                          <Box sx={fieldGridSx}>
                            {showWakePrompt ? (
                              <TextField
                                sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                                label={t('audioConfig.wakePrompt')}
                                value={form.wake_word.wake_prompt}
                                onChange={(e) =>
                                  setDraftSafe({
                                    ...form,
                                    wake_word: {
                                      ...form.wake_word,
                                      wake_prompt: e.target.value,
                                    },
                                  })
                                }
                              />
                            ) : null}
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeEnterThreshold')}
                              value={String(form.wake_word.enter_threshold)}
                              inputProps={{ step: 0.01, min: 0.01, max: 1 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    enter_threshold: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeLeaveThreshold')}
                              value={String(form.wake_word.leave_threshold)}
                              inputProps={{ step: 0.01, min: 0, max: 1 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    leave_threshold: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeReferenceSuppressRatio')}
                              value={String(form.wake_word.reference_suppress_ratio)}
                              inputProps={{ step: 0.01, min: 0.5, max: 4 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    reference_suppress_ratio: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeZcrMin')}
                              value={String(form.wake_word.zcr_min)}
                              inputProps={{ step: 0.01, min: 0, max: 1 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    zcr_min: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeZcrMax')}
                              value={String(form.wake_word.zcr_max)}
                              inputProps={{ step: 0.01, min: 0, max: 1 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    zcr_max: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeMinSpeechBandRatio')}
                              value={String(form.wake_word.min_speech_band_ratio)}
                              inputProps={{ step: 0.01, min: 0, max: 1 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    min_speech_band_ratio: v,
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeMinActiveMs')}
                              value={String(form.wake_word.min_active_ms)}
                              inputProps={{ step: 1, min: 20, max: 5000 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    min_active_ms: Math.trunc(v),
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeHangoverMs')}
                              value={String(form.wake_word.hangover_ms)}
                              inputProps={{ step: 1, min: 0, max: 10000 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    hangover_ms: Math.trunc(v),
                                  },
                                })
                              }}
                            />
                            <TextField
                              type="number"
                              label={t('audioConfig.wakeCooldownMs')}
                              value={String(form.wake_word.cooldown_ms)}
                              inputProps={{ step: 1, min: 100, max: 10000 }}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  wake_word: {
                                    ...form.wake_word,
                                    cooldown_ms: Math.trunc(v),
                                  },
                                })
                              }}
                            />
                          </Box>
                        ) : null}
                      </Box>
                    ) : null}

                    {showRealtime ? (
                      <FormSectionSubCollapsible title={t('audioConfig.sectionRealtime')}>
                        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                          {realtimeReady ? (
                            <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
                              {t('audioConfig.realtimeConfiguredHelp')}
                            </Typography>
                          ) : null}
                          <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
                            {`${t('audioConfig.realtimeSampleRateHint')} ${realtimeSampleRate} Hz`}
                          </Typography>
                          <Box sx={fieldGridSx}>
                            <FormControl fullWidth>
                              <InputLabel id="realtime-provider">{t('audioConfig.realtimeProvider')}</InputLabel>
                              <Select
                                labelId="realtime-provider"
                                label={t('audioConfig.realtimeProvider')}
                                value={form.realtime.provider}
                                onChange={(e: SelectChangeEvent) => {
                                  const provider = e.target.value
                                  const oldRequiredSampleRate = realtimeRequiredSampleRate(
                                    form.realtime.provider,
                                  )
                                  const newRequiredSampleRate =
                                    realtimeRequiredSampleRate(provider)
                                  setDraftSafe({
                                    ...form,
                                    microphone: {
                                      ...form.microphone,
                                      sample_rate:
                                        form.microphone.sample_rate === oldRequiredSampleRate
                                          ? newRequiredSampleRate
                                          : form.microphone.sample_rate,
                                    },
                                    speaker: {
                                      ...form.speaker,
                                      sample_rate:
                                        form.speaker.sample_rate === oldRequiredSampleRate
                                          ? newRequiredSampleRate
                                          : form.speaker.sample_rate,
                                    },
                                    realtime: {
                                      ...form.realtime,
                                      provider,
                                      ws_url: realtimeWsUrlAfterProviderChange(
                                        form.realtime.ws_url,
                                        form.realtime.provider,
                                        provider,
                                      ),
                                      model: realtimeModelAfterProviderChange(
                                        form.realtime.model,
                                        form.realtime.provider,
                                        provider,
                                      ),
                                      voice: realtimeVoiceAfterProviderChange(
                                        form.realtime.voice,
                                        form.realtime.provider,
                                        provider,
                                      ),
                                    },
                                  })
                                }}
                              >
                                {unionStringPreset(
                                  [...AUDIO_REALTIME_PROVIDERS],
                                  form.realtime.provider,
                                ).map((provider) => (
                                  <MenuItem key={provider} value={provider}>
                                    {(AUDIO_REALTIME_PROVIDERS as readonly string[]).includes(provider)
                                      ? t(`audioConfig.realtimeProviderLabels.${provider}`)
                                      : provider}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                            <TextField
                              label={t('audioConfig.realtimeApiKey')}
                              type={isRevealed('audio_realtime_api_key') ? 'text' : 'password'}
                              value={form.realtime.api_key}
                              onChange={(e) =>
                                setDraftSafe({
                                  ...form,
                                  realtime: { ...form.realtime, api_key: e.target.value },
                                })
                              }
                              slotProps={{
                                htmlInput: {
                                  style: { fontFamily: 'var(--font-mono)' },
                                  ...getRevealHandlers('audio_realtime_api_key'),
                                },
                              }}
                            />
                            <TextField
                              label={t('audioConfig.realtimeModel')}
                              value={form.realtime.model}
                              onChange={(e) =>
                                setDraftSafe({
                                  ...form,
                                  realtime: {
                                    ...form.realtime,
                                    model: e.target.value.trim(),
                                  },
                                })
                              }
                            />
                            <TextField
                              label={t('audioConfig.realtimeVoice')}
                              value={form.realtime.voice}
                              onChange={(e) =>
                                setDraftSafe({
                                  ...form,
                                  realtime: {
                                    ...form.realtime,
                                    voice: e.target.value.trim(),
                                  },
                                })
                              }
                            />
                          </Box>
                          <Box sx={fieldGridSx}>
                            <TextField
                              sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                              label={t('audioConfig.realtimeWsUrl')}
                              value={form.realtime.ws_url}
                              onChange={(e) =>
                                setDraftSafe({
                                  ...form,
                                  realtime: {
                                    ...form.realtime,
                                  ws_url: e.target.value.trim(),
                                  },
                                })
                              }
                            />
                            <TextField
                              multiline
                              minRows={3}
                              sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                              label={t('audioConfig.realtimeInstructions')}
                              helperText={t('audioConfig.realtimeInstructionsHelp')}
                              value={form.realtime.instructions}
                              onChange={(e) =>
                                setDraftSafe({
                                  ...form,
                                  realtime: {
                                    ...form.realtime,
                                    instructions: e.target.value,
                                  },
                                })
                              }
                            />
                          </Box>
                        </Box>
                      </FormSectionSubCollapsible>
                    ) : null}
                  </>
                )}

              {activeAudioTab === 2 && (showAmbientBlock || showLedBlock) ? (
                <>
                  {showAmbientBlock ? (
                    <FormSectionSub title={t('audioConfig.sectionAmbient')}>
                      <Box sx={fieldGridSx}>
                        <FormControlLabel
                          sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                          control={
                            <Switch
                              checked={form.ambient_listening.enabled}
                              onChange={(_, checked) =>
                                setDraftSafe({
                                  ...form,
                                  ambient_listening: {
                                    ...form.ambient_listening,
                                    enabled: checked,
                                  },
                                })
                              }
                            />
                          }
                          label={t('audioConfig.ambientEnabled')}
                        />
                        {form.ambient_listening.enabled ? (
                          <>
                            <FormControlLabel
                              sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                              control={
                                <Switch
                                  checked={form.ambient_listening.detect_emotions}
                                  onChange={(_, checked) =>
                                    setDraftSafe({
                                      ...form,
                                      ambient_listening: {
                                        ...form.ambient_listening,
                                        detect_emotions: checked,
                                      },
                                    })
                                  }
                                />
                              }
                              label={t('audioConfig.ambientDetectEmotions')}
                            />
                            <Box sx={{ gridColumn: '1 / -1' }}>
                              <Typography variant="caption" display="block" sx={{ color: "var(--text-tertiary)" }}>
                                {t('audioConfig.soundEventsPick')}
                              </Typography>
                              <FormGroup row sx={{ flexWrap: 'wrap', gap: 0.5, mt: 0.5 }}>
                                {AUDIO_AMBIENT_SOUND_EVENT_PRESETS.map((ev) => (
                                  <FormControlLabel
                                    key={ev}
                                    control={
                                      <Checkbox
                                        checked={presetSoundEventsSelected(
                                          form.ambient_listening.sound_events,
                                        ).has(ev)}
                                        onChange={() => togglePresetSoundEvent(ev)}
                                      />
                                    }
                                    label={t(`audioConfig.soundEventLabels.${ev}`)}
                                  />
                                ))}
                              </FormGroup>
                              <TextField
                                fullWidth
                                sx={{ mt: 1 }}
                                label={t('audioConfig.ambientSoundEventsExtra')}
                                helperText={t('audioConfig.ambientSoundEventsExtraHelp')}
                                value={extrasStr}
                                onChange={(e) => setExtraSoundEventsStr(e.target.value)}
                              />
                            </Box>
                            <FormControl fullWidth>
                              <InputLabel id="amb-cool">{t('audioConfig.ambientCooldownMinutes')}</InputLabel>
                              <Select
                                labelId="amb-cool"
                                label={t('audioConfig.ambientCooldownMinutes')}
                                value={String(form.ambient_listening.cooldown_minutes)}
                                onChange={(e: SelectChangeEvent) => {
                                  const v = asNumber(e.target.value)
                                  if (v == null) return
                                  setDraftSafe({
                                    ...form,
                                    ambient_listening: {
                                      ...form.ambient_listening,
                                      cooldown_minutes: Math.trunc(v),
                                    },
                                  })
                                }}
                              >
                                {ambientCooldownOptions(form.ambient_listening.cooldown_minutes).map(
                                  (m) => (
                                    <MenuItem key={m} value={String(m)}>
                                      {m}
                                    </MenuItem>
                                  ),
                                )}
                              </Select>
                            </FormControl>
                            <FormControl fullWidth>
                              <InputLabel id="amb-int">{t('audioConfig.ambientCheckIntervalSeconds')}</InputLabel>
                              <Select
                                labelId="amb-int"
                                label={t('audioConfig.ambientCheckIntervalSeconds')}
                                value={String(form.ambient_listening.check_interval_seconds)}
                                onChange={(e: SelectChangeEvent) => {
                                  const v = asNumber(e.target.value)
                                  if (v == null) return
                                  setDraftSafe({
                                    ...form,
                                    ambient_listening: {
                                      ...form.ambient_listening,
                                      check_interval_seconds: Math.trunc(v),
                                    },
                                  })
                                }}
                              >
                                {ambientIntervalOptions(
                                  form.ambient_listening.check_interval_seconds,
                                ).map((s) => (
                                  <MenuItem key={s} value={String(s)}>
                                    {s} s
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                          </>
                        ) : null}
                      </Box>
                    </FormSectionSub>
                  ) : null}

                  {showLedBlock ? (
                    <FormSectionSub title={t('audioConfig.sectionLed')}>
                      <Box sx={fieldGridSx}>
                        <FormControlLabel
                          sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                          control={
                            <Switch
                              checked={form.led_indicator.enabled}
                              onChange={(_, checked) =>
                                setDraftSafe({
                                  ...form,
                                  led_indicator: { ...form.led_indicator, enabled: checked },
                                })
                              }
                            />
                          }
                          label={t('audioConfig.ledEnabled')}
                        />
                        {form.led_indicator.enabled ? (
                          <>
                            <TextField
                              label={t('audioConfig.ledPin')}
                              value={String(form.led_indicator.pin)}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  led_indicator: { ...form.led_indicator, pin: Math.trunc(v) },
                                })
                              }}
                            />
                            <FormControl fullWidth>
                              <InputLabel id="led-l">{t('audioConfig.ledListening')}</InputLabel>
                              <Select
                                labelId="led-l"
                                label={t('audioConfig.ledListening')}
                                value={form.led_indicator.states.listening}
                                onChange={(e: SelectChangeEvent) =>
                                  setDraftSafe({
                                    ...form,
                                    led_indicator: {
                                      ...form.led_indicator,
                                      states: {
                                        ...form.led_indicator.states,
                                        listening: e.target.value,
                                      },
                                    },
                                  })
                                }
                              >
                                {unionStringPreset(
                                  [...AUDIO_LED_STATE_PRESETS],
                                  form.led_indicator.states.listening,
                                ).map((st) => (
                                  <MenuItem key={st} value={st}>
                                    {(AUDIO_LED_STATE_PRESETS as readonly string[]).includes(st)
                                      ? t(`audioConfig.ledStateLabels.${st}`)
                                      : st}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                            <FormControl fullWidth>
                              <InputLabel id="led-p">{t('audioConfig.ledProcessing')}</InputLabel>
                              <Select
                                labelId="led-p"
                                label={t('audioConfig.ledProcessing')}
                                value={form.led_indicator.states.processing}
                                onChange={(e: SelectChangeEvent) =>
                                  setDraftSafe({
                                    ...form,
                                    led_indicator: {
                                      ...form.led_indicator,
                                      states: {
                                        ...form.led_indicator.states,
                                        processing: e.target.value,
                                      },
                                    },
                                  })
                                }
                              >
                                {unionStringPreset(
                                  [...AUDIO_LED_STATE_PRESETS],
                                  form.led_indicator.states.processing,
                                ).map((st) => (
                                  <MenuItem key={st} value={st}>
                                    {(AUDIO_LED_STATE_PRESETS as readonly string[]).includes(st)
                                      ? t(`audioConfig.ledStateLabels.${st}`)
                                      : st}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                            <FormControl fullWidth>
                              <InputLabel id="led-s">{t('audioConfig.ledSpeaking')}</InputLabel>
                              <Select
                                labelId="led-s"
                                label={t('audioConfig.ledSpeaking')}
                                value={form.led_indicator.states.speaking}
                                onChange={(e: SelectChangeEvent) =>
                                  setDraftSafe({
                                    ...form,
                                    led_indicator: {
                                      ...form.led_indicator,
                                      states: {
                                        ...form.led_indicator.states,
                                        speaking: e.target.value,
                                      },
                                    },
                                  })
                                }
                              >
                                {unionStringPreset(
                                  [...AUDIO_LED_STATE_PRESETS],
                                  form.led_indicator.states.speaking,
                                ).map((st) => (
                                  <MenuItem key={st} value={st}>
                                    {(AUDIO_LED_STATE_PRESETS as readonly string[]).includes(st)
                                      ? t(`audioConfig.ledStateLabels.${st}`)
                                      : st}
                                  </MenuItem>
                                ))}
                              </Select>
                            </FormControl>
                          </>
                        ) : null}
                      </Box>
                    </FormSectionSub>
                  ) : null}
                </>
              ) : null}
              </Box>
            </>
          ) : null}
        </FormFieldStack>
      </SettingsSection>
    </Box>
  )
}
