import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Accordion from '@mui/material/Accordion'
import AccordionDetails from '@mui/material/AccordionDetails'
import AccordionSummary from '@mui/material/AccordionSummary'
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
import ExpandMoreRounded from '@mui/icons-material/ExpandMoreRounded'
import SaveRounded from '@mui/icons-material/SaveRounded'
import HearingRounded from '@mui/icons-material/HearingRounded'
import type { SelectChangeEvent } from '@mui/material/Select'
import {
  FormFieldStack,
  FormLoadingSkeleton,
  FormSectionSub,
  InlineAlert,
  SaveFeedback,
} from '../components/form'
import { SettingsSection } from '../components/SettingsSection'
import { useConfig } from '../hooks/useConfig'
import { useRevealedPasswordFields } from '../hooks/useRevealedPassword'
import { useSaveFeedback } from '../hooks/useSaveFeedback'
import { useUnsaved } from '../hooks/useUnsaved'
import {
  AUDIO_AMBIENT_SOUND_EVENT_PRESETS,
  AUDIO_BITS_PER_SAMPLE_ALLOWED,
  AUDIO_BUFFER_SIZE_MAX,
  AUDIO_BUFFER_SIZE_MIN,
  AUDIO_CONFIG_VERSION,
  AUDIO_LED_STATE_PRESETS,
  AUDIO_MIC_DEVICE_TYPES,
  AUDIO_PIN_MAX,
  AUDIO_PIN_MIN,
  AUDIO_SAMPLE_RATE_MAX,
  AUDIO_SAMPLE_RATE_MIN,
  AUDIO_SPEAKER_DEVICE_TYPES,
  AUDIO_STT_LANGUAGES,
  AUDIO_STT_PROVIDERS,
  AUDIO_TTS_EDGE_VOICES,
  AUDIO_TTS_PITCH_PRESETS,
  AUDIO_TTS_PROVIDERS,
  AUDIO_TTS_RATE_PRESETS,
  AUDIO_VAD_SILENCE_MS_PRESETS,
  AUDIO_VAD_THRESHOLD_PRESETS,
  ambientCooldownOptions,
  ambientIntervalOptions,
  bufferSelectOptions,
  DEFAULT_STT_API_URL_BAIDU,
  DEFAULT_STT_API_URL_WHISPER,
  defaultAudioConfig,
  normalizeAudioConfigForSave,
  sampleRateSelectOptions,
  unionFloatPreset,
  unionNumberPreset,
  unionStringPreset,
  type AudioConfig,
} from '../types/audioConfig'

function asNumber(v: string): number | null {
  const n = Number(v)
  return Number.isFinite(n) ? n : null
}

function ttsEdgeVoiceOptions(current: string): string[] {
  return unionStringPreset([...AUDIO_TTS_EDGE_VOICES], current)
}

function validateAudioConfig(
  form: AudioConfig,
  t: (k: string) => string,
): string | null {
  if (form.version !== AUDIO_CONFIG_VERSION) {
    return t('audioConfig.validation.version')
  }
  const pinInRange = (p: number) => p >= AUDIO_PIN_MIN && p <= AUDIO_PIN_MAX
  const srInRange = (v: number) => v >= AUDIO_SAMPLE_RATE_MIN && v <= AUDIO_SAMPLE_RATE_MAX
  const bitsValid = (v: number) => (AUDIO_BITS_PER_SAMPLE_ALLOWED as readonly number[]).includes(v)

  if (form.microphone.enabled) {
    if (
      !pinInRange(form.microphone.pins.ws) ||
      !pinInRange(form.microphone.pins.sck) ||
      !pinInRange(form.microphone.pins.din)
    ) {
      return t('audioConfig.validation.pin')
    }
    if (!srInRange(form.microphone.sample_rate)) {
      return t('audioConfig.validation.sampleRate')
    }
    if (!bitsValid(form.microphone.bits_per_sample)) {
      return t('audioConfig.validation.bitsPerSample')
    }
    if (
      form.microphone.buffer_size < AUDIO_BUFFER_SIZE_MIN ||
      form.microphone.buffer_size > AUDIO_BUFFER_SIZE_MAX
    ) {
      return t('audioConfig.validation.bufferSize')
    }
  }

  if (form.speaker.enabled) {
    if (
      !pinInRange(form.speaker.pins.ws) ||
      !pinInRange(form.speaker.pins.sck) ||
      !pinInRange(form.speaker.pins.dout)
    ) {
      return t('audioConfig.validation.pin')
    }
    if (form.speaker.pins.sd != null && !pinInRange(form.speaker.pins.sd)) {
      return t('audioConfig.validation.pin')
    }
    if (!srInRange(form.speaker.sample_rate)) {
      return t('audioConfig.validation.sampleRate')
    }
    if (!bitsValid(form.speaker.bits_per_sample)) {
      return t('audioConfig.validation.bitsPerSample')
    }
  }

  if (form.vad.threshold < 0 || form.vad.threshold > 1) {
    return t('audioConfig.validation.vadThreshold')
  }
  if (form.vad.silence_duration_ms < 1 || form.vad.silence_duration_ms > 60_000) {
    return t('audioConfig.validation.vadSilence')
  }

  if (form.ambient_listening.sound_events.length > 16) {
    return t('audioConfig.validation.soundEvents')
  }
  if (form.ambient_listening.sound_events.some((s) => !s.trim() || s.length > 32)) {
    return t('audioConfig.validation.soundEvents')
  }
  if (
    form.ambient_listening.check_interval_seconds < 1 ||
    form.ambient_listening.check_interval_seconds > 86_400
  ) {
    return t('audioConfig.validation.checkInterval')
  }
  if (form.led_indicator.enabled && !pinInRange(form.led_indicator.pin)) {
    return t('audioConfig.validation.pin')
  }
  if (
    form.enabled &&
    form.microphone.enabled &&
    form.stt.provider === 'baidu' &&
    (!form.stt.api_key.trim() || !form.stt.api_secret.trim())
  ) {
    return t('audioConfig.validation.sttCredentialRequired')
  }
  return null
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

export function AudioConfigPanel() {
  const { t } = useTranslation()
  const {
    audioConfig,
    audioLoading,
    audioError,
    loadAudioConfig,
    saveAudioConfig,
  } = useConfig()
  const saveFeedback = useSaveFeedback(t)
  const { setDirty } = useUnsaved()
  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields()
  const [draft, setDraft] = useState<AudioConfig | null>(null)
  const [audioTab, setAudioTab] = useState(0)
  const form = draft ?? audioConfig ?? defaultAudioConfig()

  useEffect(() => {
    void loadAudioConfig()
  }, [loadAudioConfig])

  useEffect(() => {
    if (!form.enabled) setAudioTab(0)
  }, [form.enabled])

  const saveDisabled = saveFeedback.status === 'saving'
  const setDraftSafe = (next: AudioConfig) => {
    setDirty(true)
    setDraft(next)
  }

  if (audioLoading && !audioConfig && !draft) {
    return (
      <SettingsSection
        icon={<HearingRounded sx={{ fontSize: 'var(--icon-size-md)' }} />}
        label={t('audioConfig.sectionMain')}
      >
        <FormLoadingSkeleton />
      </SettingsSection>
    )
  }

  const save = async () => {
    const err = validateAudioConfig(form, t)
    if (err) {
      saveFeedback.fail(err)
      return
    }
    saveFeedback.begin()
    const payload = normalizeAudioConfigForSave(form)
    const result = await saveAudioConfig(payload)
    saveFeedback.finishFromResult(result)
    if (result.ok) setDirty(false)
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
  const micOn = audioOn && form.microphone.enabled
  const spkOn = audioOn && form.speaker.enabled
  const showVadWake = micOn
  const showStt = micOn
  const showTts = spkOn
  const showAmbientBlock = micOn
  const showLedBlock = audioOn

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

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <InlineAlert message={audioError} onRetry={loadAudioConfig} />
      <SettingsSection
        icon={<HearingRounded sx={{ fontSize: 'var(--icon-size-md)' }} />}
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
            {saveFeedback.status === 'saving' ? t('common.saving') : t('common.save')}
          </Button>
        }
        belowTitleRow={
          saveFeedback.status === 'ok' || saveFeedback.status === 'fail' ? (
            <SaveFeedback
              placement="belowTitle"
              status={saveFeedback.status}
              message={
                saveFeedback.status === 'ok'
                  ? t('audioConfig.restartRequired')
                  : saveFeedback.error
              }
              autoDismissMs={3000}
              onDismiss={saveFeedback.dismiss}
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
              <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
                {t('audioConfig.hintEnableAudioFirst')}
              </Typography>
            ) : null}
          </FormSectionSub>

          {audioOn ? (
            <>
              <Tabs
                value={audioTab}
                onChange={(_, v) => setAudioTab(v)}
                variant="scrollable"
                allowScrollButtonsMobile
                sx={{
                  borderBottom: 1,
                  borderColor: 'var(--border-subtle)',
                  minHeight: 44,
                  '& .MuiTab-root': {
                    minHeight: 44,
                    textTransform: 'none',
                    fontSize: '0.875rem',
                  },
                }}
              >
                <Tab label={t('audioConfig.tabMic')} />
                <Tab label={t('audioConfig.tabSpeaker')} />
                <Tab label={t('audioConfig.tabSpeech')} />
                <Tab label={t('audioConfig.tabMore')} />
              </Tabs>
              <Box sx={{ pt: 2.5 }}>
                {audioTab === 0 && (
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
                    <FormControl size="small" fullWidth>
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
                    <FormControl size="small" fullWidth>
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
                    <Accordion
                      defaultExpanded={false}
                      disableGutters
                      elevation={0}
                      sx={{
                        '&:before': { display: 'none' },
                        border: '1px solid var(--border-subtle)',
                        borderRadius: 'var(--radius-card)',
                        boxShadow: 'none',
                        '&.Mui-expanded': { m: 0 },
                      }}
                    >
                      <AccordionSummary expandIcon={<ExpandMoreRounded />}>
                        <Typography variant="body2" sx={{ fontWeight: 600 }}>
                          {t('audioConfig.accordionMicHw')}
                        </Typography>
                      </AccordionSummary>
                      <AccordionDetails sx={{ pt: 0, px: 0 }}>
                        <Box sx={fieldGridSx}>
                    <FormControl size="small" fullWidth>
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
                    <FormControl size="small" fullWidth>
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
                    <TextField
                      size="small"
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
                      size="small"
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
                      size="small"
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
                      </AccordionDetails>
                    </Accordion>
                  </Box>
                ) : null}
              </FormSectionSub>
                )}

                {audioTab === 1 && (
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
                      <FormControl size="small" fullWidth>
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
                            [...AUDIO_SPEAKER_DEVICE_TYPES],
                            form.speaker.device_type,
                          ).map((dt) => (
                            <MenuItem key={dt} value={dt}>
                              {(AUDIO_SPEAKER_DEVICE_TYPES as readonly string[]).includes(dt)
                                ? t(`audioConfig.deviceSpeaker.${dt}`)
                                : dt}
                            </MenuItem>
                          ))}
                        </Select>
                      </FormControl>
                      <FormControl size="small" fullWidth>
                        <InputLabel id="spk-sr">{t('audioConfig.sampleRate')}</InputLabel>
                        <Select
                          labelId="spk-sr"
                          label={t('audioConfig.sampleRate')}
                          value={String(form.speaker.sample_rate)}
                          onChange={(e: SelectChangeEvent) => {
                            const v = asNumber(e.target.value)
                            if (v == null) return
                            setDraftSafe({
                              ...form,
                              speaker: { ...form.speaker, sample_rate: Math.trunc(v) },
                            })
                          }}
                        >
                          {sampleRateSelectOptions(form.speaker.sample_rate).map((sr) => (
                            <MenuItem key={sr} value={String(sr)}>
                              {sr} Hz
                            </MenuItem>
                          ))}
                        </Select>
                      </FormControl>
                      <FormControl size="small" fullWidth>
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
                    </Box>
                    <Accordion
                      defaultExpanded={false}
                      disableGutters
                      elevation={0}
                      sx={{
                        '&:before': { display: 'none' },
                        border: '1px solid var(--border-subtle)',
                        borderRadius: 'var(--radius-card)',
                        boxShadow: 'none',
                        '&.Mui-expanded': { m: 0 },
                      }}
                    >
                      <AccordionSummary expandIcon={<ExpandMoreRounded />}>
                        <Typography variant="body2" sx={{ fontWeight: 600 }}>
                          {t('audioConfig.accordionSpkHw')}
                        </Typography>
                      </AccordionSummary>
                      <AccordionDetails sx={{ pt: 0, px: 0 }}>
                          <Box sx={fieldGridSx}>
                            <TextField
                              size="small"
                              label={t('audioConfig.pinWs')}
                              value={String(form.speaker.pins.ws)}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  speaker: {
                                    ...form.speaker,
                                    pins: { ...form.speaker.pins, ws: Math.trunc(v) },
                                  },
                                })
                              }}
                            />
                            <TextField
                              size="small"
                              label={t('audioConfig.pinSck')}
                              value={String(form.speaker.pins.sck)}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  speaker: {
                                    ...form.speaker,
                                    pins: { ...form.speaker.pins, sck: Math.trunc(v) },
                                  },
                                })
                              }}
                            />
                            <TextField
                              size="small"
                              label={t('audioConfig.pinDout')}
                              value={String(form.speaker.pins.dout)}
                              onChange={(e) => {
                                const v = asNumber(e.target.value)
                                if (v == null) return
                                setDraftSafe({
                                  ...form,
                                  speaker: {
                                    ...form.speaker,
                                    pins: { ...form.speaker.pins, dout: Math.trunc(v) },
                                  },
                                })
                              }}
                            />
                            <TextField
                              size="small"
                              label={t('audioConfig.pinSdOptional')}
                              placeholder={t('audioConfig.pinOptionalPlaceholder')}
                              value={form.speaker.pins.sd != null ? String(form.speaker.pins.sd) : ''}
                              onChange={(e) => {
                                const raw = e.target.value.trim()
                                if (raw === '') {
                                  setDraftSafe({
                                    ...form,
                                    speaker: {
                                      ...form.speaker,
                                      pins: { ...form.speaker.pins, sd: null },
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
                                    pins: { ...form.speaker.pins, sd: Math.trunc(v) },
                                  },
                                })
                              }}
                            />
                          </Box>
                      </AccordionDetails>
                    </Accordion>
                  </Box>
                ) : null}
              </FormSectionSub>
                )}

                {audioTab === 2 && (
                <>
              {showVadWake ? (
                <FormSectionSub title={t('audioConfig.sectionVadWakeWord')}>
                  <Box sx={fieldGridSx}>
                        <FormControl size="small" fullWidth>
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
                        <FormControl size="small" fullWidth>
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
                                vad: { ...form.vad, silence_duration_ms: Math.trunc(v) },
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
                    <FormControlLabel
                      sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
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
                      <TextField
                        size="small"
                        sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                        label={t('audioConfig.wakeWordKeyword')}
                        value={form.wake_word.keyword}
                        onChange={(e) =>
                          setDraftSafe({
                            ...form,
                            wake_word: { ...form.wake_word, keyword: e.target.value.trim() },
                          })
                        }
                      />
                    ) : null}
                  </Box>
                </FormSectionSub>
              ) : null}

              {showStt || showTts ? (
                <FormSectionSub title={t('audioConfig.sectionSttTts')}>
                  {showStt ? (
                    <Box sx={{ ...fieldGridSx, mb: showTts ? 3 : 0 }}>
                      <Typography
                        variant="subtitle2"
                        color="text.secondary"
                        sx={{ gridColumn: '1 / -1' }}
                      >
                        {t('audioConfig.sttBlockTitle')}
                      </Typography>
                      <FormControl size="small" fullWidth>
                        <InputLabel id="stt-prov">{t('audioConfig.sttProvider')}</InputLabel>
                        <Select
                          labelId="stt-prov"
                          label={t('audioConfig.sttProvider')}
                          value={form.stt.provider}
                          onChange={(e: SelectChangeEvent) => {
                            const p = e.target.value
                            setDraftSafe({
                              ...form,
                              stt: {
                                ...form.stt,
                                provider: p,
                                api_url:
                                  p === 'whisper'
                                    ? form.stt.api_url.trim()
                                      ? form.stt.api_url
                                      : DEFAULT_STT_API_URL_WHISPER
                                    : p === 'baidu'
                                      ? form.stt.api_url.trim()
                                        ? form.stt.api_url
                                        : DEFAULT_STT_API_URL_BAIDU
                                    : form.stt.api_url,
                                model:
                                  p === 'whisper'
                                    ? 'whisper-1'
                                    : p === 'baidu'
                                      ? form.stt.model.trim()
                                        ? form.stt.model
                                        : '1537'
                                      : form.stt.model,
                              },
                            })
                          }}
                        >
                          {unionStringPreset([...AUDIO_STT_PROVIDERS], form.stt.provider).map(
                            (p) => (
                              <MenuItem key={p} value={p}>
                                {(AUDIO_STT_PROVIDERS as readonly string[]).includes(p)
                                  ? t(`audioConfig.sttProviderLabels.${p}`)
                                  : p}
                              </MenuItem>
                            ),
                          )}
                        </Select>
                      </FormControl>
                      <FormControl size="small" fullWidth>
                        <InputLabel id="stt-lang">{t('audioConfig.sttLanguage')}</InputLabel>
                        <Select
                          labelId="stt-lang"
                          label={t('audioConfig.sttLanguage')}
                          value={form.stt.language}
                          onChange={(e: SelectChangeEvent) =>
                            setDraftSafe({
                              ...form,
                              stt: { ...form.stt, language: e.target.value },
                            })
                          }
                        >
                          {unionStringPreset([...AUDIO_STT_LANGUAGES], form.stt.language).map(
                            (lang) => (
                              <MenuItem key={lang} value={lang}>
                                {(AUDIO_STT_LANGUAGES as readonly string[]).includes(lang)
                                  ? t(`audioConfig.sttLangLabels.${lang}`)
                                  : lang}
                              </MenuItem>
                            ),
                          )}
                        </Select>
                      </FormControl>
                      {form.stt.provider !== 'whisper' ? (
                        <TextField
                          size="small"
                          sx={{ gridColumn: { xs: '1', md: '1 / -1' } }}
                          label={t('audioConfig.sttApiUrl')}
                          value={form.stt.api_url}
                          onChange={(e) =>
                            setDraftSafe({
                              ...form,
                              stt: { ...form.stt, api_url: e.target.value.trim() },
                            })
                          }
                        />
                      ) : null}
                      <TextField
                        size="small"
                        label={t('audioConfig.sttApiKey')}
                        type={isRevealed('audio_stt_api_key') ? 'text' : 'password'}
                        value={form.stt.api_key}
                        onChange={(e) =>
                          setDraftSafe({ ...form, stt: { ...form.stt, api_key: e.target.value } })
                        }
                        slotProps={{
                          htmlInput: {
                            style: { fontFamily: 'var(--font-mono)' },
                            ...getRevealHandlers('audio_stt_api_key'),
                          },
                        }}
                      />
                      {form.stt.provider === 'baidu' ? (
                        <TextField
                          size="small"
                          label={t('audioConfig.sttApiSecret')}
                          type={isRevealed('audio_stt_api_secret') ? 'text' : 'password'}
                          value={form.stt.api_secret}
                          onChange={(e) =>
                            setDraftSafe({
                              ...form,
                              stt: { ...form.stt, api_secret: e.target.value },
                            })
                          }
                          slotProps={{
                            htmlInput: {
                              style: { fontFamily: 'var(--font-mono)' },
                              ...getRevealHandlers('audio_stt_api_secret'),
                            },
                          }}
                        />
                      ) : null}
                      {form.stt.provider !== 'whisper' ? (
                        <TextField
                          size="small"
                          label={t('audioConfig.sttModel')}
                          value={form.stt.model}
                          onChange={(e) =>
                            setDraftSafe({
                              ...form,
                              stt: { ...form.stt, model: e.target.value.trim() },
                            })
                          }
                        />
                      ) : null}
                    </Box>
                  ) : null}
                  {showTts ? (
                    <Box sx={fieldGridSx}>
                      <Typography
                        variant="subtitle2"
                        color="text.secondary"
                        sx={{ gridColumn: '1 / -1' }}
                      >
                        {t('audioConfig.ttsBlockTitle')}
                      </Typography>
                      <FormControl size="small" fullWidth>
                        <InputLabel id="tts-prov">{t('audioConfig.ttsProvider')}</InputLabel>
                        <Select
                          labelId="tts-prov"
                          label={t('audioConfig.ttsProvider')}
                          value={form.tts.provider}
                          onChange={(e: SelectChangeEvent) => {
                            const p = e.target.value
                            setDraftSafe({
                              ...form,
                              tts: {
                                ...form.tts,
                                provider: p,
                                voice:
                                  p === 'edge' && !form.tts.voice.trim()
                                    ? 'zh-CN-XiaoxiaoNeural'
                                    : form.tts.voice,
                              },
                            })
                          }}
                        >
                          {unionStringPreset([...AUDIO_TTS_PROVIDERS], form.tts.provider).map(
                            (p) => (
                              <MenuItem key={p} value={p}>
                                {(AUDIO_TTS_PROVIDERS as readonly string[]).includes(p)
                                  ? t(`audioConfig.ttsProviderLabels.${p}`)
                                  : p}
                              </MenuItem>
                            ),
                          )}
                        </Select>
                      </FormControl>
                      {form.tts.provider === 'edge' ? (
                        <>
                          <FormControl size="small" fullWidth>
                            <InputLabel id="tts-voice">{t('audioConfig.ttsVoice')}</InputLabel>
                            <Select
                              labelId="tts-voice"
                              label={t('audioConfig.ttsVoice')}
                              value={form.tts.voice}
                              onChange={(e: SelectChangeEvent) =>
                                setDraftSafe({
                                  ...form,
                                  tts: { ...form.tts, voice: e.target.value },
                                })
                              }
                            >
                              {ttsEdgeVoiceOptions(form.tts.voice).map((v) => (
                                <MenuItem key={v} value={v}>
                                  {v}
                                </MenuItem>
                              ))}
                            </Select>
                          </FormControl>
                          <FormControl size="small" fullWidth>
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
                              {unionStringPreset([...AUDIO_TTS_RATE_PRESETS], form.tts.rate).map(
                                (r) => (
                                  <MenuItem key={r} value={r}>
                                    {r}
                                  </MenuItem>
                                ),
                              )}
                            </Select>
                          </FormControl>
                          <FormControl size="small" fullWidth>
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
                              {unionStringPreset([...AUDIO_TTS_PITCH_PRESETS], form.tts.pitch).map(
                                (p) => (
                                  <MenuItem key={p} value={p}>
                                    {p}
                                  </MenuItem>
                                ),
                              )}
                            </Select>
                          </FormControl>
                        </>
                      ) : (
                        <>
                          <TextField
                            size="small"
                            label={t('audioConfig.ttsVoice')}
                            value={form.tts.voice}
                            onChange={(e) =>
                              setDraftSafe({
                                ...form,
                                tts: { ...form.tts, voice: e.target.value.trim() },
                              })
                            }
                          />
                          <TextField
                            size="small"
                            label={t('audioConfig.ttsRate')}
                            value={form.tts.rate}
                            onChange={(e) =>
                              setDraftSafe({
                                ...form,
                                tts: { ...form.tts, rate: e.target.value.trim() },
                              })
                            }
                          />
                          <TextField
                            size="small"
                            label={t('audioConfig.ttsPitch')}
                            value={form.tts.pitch}
                            onChange={(e) =>
                              setDraftSafe({
                                ...form,
                                tts: { ...form.tts, pitch: e.target.value.trim() },
                              })
                            }
                          />
                        </>
                      )}
                    </Box>
                  ) : null}
                </FormSectionSub>
              ) : null}
                </>
                )}

              {audioTab === 3 && (showAmbientBlock || showLedBlock) ? (
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
                              <Typography variant="caption" color="text.secondary" display="block">
                                {t('audioConfig.soundEventsPick')}
                              </Typography>
                              <FormGroup row sx={{ flexWrap: 'wrap', gap: 0.5, mt: 0.5 }}>
                                {AUDIO_AMBIENT_SOUND_EVENT_PRESETS.map((ev) => (
                                  <FormControlLabel
                                    key={ev}
                                    control={
                                      <Checkbox
                                        size="small"
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
                                size="small"
                                fullWidth
                                sx={{ mt: 1 }}
                                label={t('audioConfig.ambientSoundEventsExtra')}
                                helperText={t('audioConfig.ambientSoundEventsExtraHelp')}
                                value={extrasStr}
                                onChange={(e) => setExtraSoundEventsStr(e.target.value)}
                              />
                            </Box>
                            <FormControl size="small" fullWidth>
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
                            <FormControl size="small" fullWidth>
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
                              size="small"
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
                            <FormControl size="small" fullWidth>
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
                            <FormControl size="small" fullWidth>
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
                            <FormControl size="small" fullWidth>
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
