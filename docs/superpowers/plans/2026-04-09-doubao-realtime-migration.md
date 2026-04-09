# Doubao Realtime Voice Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Beetle's Baidu realtime voice path with Doubao realtime voice, and remove Baidu realtime runtime/config/configure-ui branches in one pass.

**Architecture:** Keep the existing `voice_session -> audio::realtime -> WSS -> voice-exclusive / external WSS suspend` runtime architecture. Change only the realtime provider contract, protocol adapter, config validation, and configure-ui surface so Beetle runs a single Doubao-specific realtime path instead of carrying Baidu's separate 16k/license/protocol fork.

**Tech Stack:** Rust, serde/serde_json, current WSS abstraction, configure-ui React/TypeScript, cargo test/check, npm test/build where applicable.

---

### Task 1: Lock The Realtime Provider Contract To Doubao

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs`
- Modify: `configure-ui/src/types/audioConfig.ts`
- Modify: `configure-ui/src/pages/AudioConfigPanel.tsx`
- Modify: `configure-ui/src/i18n/locales/zh-CN.ts`
- Modify: `configure-ui/src/i18n/locales/en-US.ts`

- [ ] **Step 1: Write the failing Rust config tests**

Add tests covering:

```rust
#[test]
fn doubao_realtime_validation_requires_model_voice_and_api_key() {
    let mut seg = default_disabled_audio_segment();
    seg.enabled = true;
    seg.microphone.enabled = true;
    seg.speaker.enabled = true;
    seg.wake_word.enabled = true;
    seg.realtime.provider = AUDIO_REALTIME_PROVIDER_DOUBAO.to_string();
    seg.realtime.ws_url = audio_realtime_default_ws_url(AUDIO_REALTIME_PROVIDER_DOUBAO).to_string();
    seg.realtime.api_key = "key".to_string();
    seg.realtime.model = "doubao-realtime".to_string();
    seg.realtime.voice = "zh_female".to_string();
    seg.microphone.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
    seg.speaker.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;

    assert!(validate_audio_segment(&seg).is_ok());
}
```

```rust
#[test]
fn realtime_provider_validation_rejects_removed_baidu_provider() {
    let mut seg = default_disabled_audio_segment();
    seg.enabled = true;
    seg.microphone.enabled = true;
    seg.speaker.enabled = true;
    seg.wake_word.enabled = true;
    seg.realtime.provider = "baidu".to_string();
    seg.realtime.ws_url = "wss://example.invalid/realtime".to_string();
    seg.realtime.api_key = "key".to_string();
    seg.realtime.model = "ignored".to_string();
    seg.realtime.voice = "ignored".to_string();
    seg.microphone.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
    seg.speaker.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;

    let error = validate_audio_segment(&seg).expect_err("baidu realtime should be rejected");
    assert!(error.to_string().contains("doubao"));
}
```

- [ ] **Step 2: Run the failing Rust tests**

Run:

```bash
cargo test --lib doubao_realtime_validation_requires_model_voice_and_api_key
cargo test --lib realtime_provider_validation_rejects_removed_baidu_provider
```

Expected: FAIL because config only knows `openai_compatible / qwen / baidu`.

- [ ] **Step 3: Write the failing configure-ui tests**

Add tests in the existing configure-ui test surface covering:

```ts
it('supports doubao realtime provider defaults and removes baidu', () => {
  expect(audioRealtimeProviderSupported('doubao')).toBe(true)
  expect(audioRealtimeProviderSupported('baidu')).toBe(false)
  expect(realtimeRequiredSampleRate('doubao')).toBe(24000)
})
```

```ts
it('audio panel hides baidu-only realtime fields for doubao provider', () => {
  // render AudioConfigPanel with realtime.provider='doubao'
  // assert API key / model / voice / ws_url are shown
  // assert baidu-only app_id / api_secret / user_id / device_id / license_key are absent
})
```

- [ ] **Step 4: Run the failing configure-ui tests**

Run the targeted UI tests with the repository's existing test command, for example:

```bash
cd configure-ui && npm test -- --runInBand audioConfig
```

Expected: FAIL because configure-ui still exposes `baidu` provider branches and labels.

- [ ] **Step 5: Implement the minimal provider-contract change**

Implement:

1. `src/config.rs`
   - replace `AUDIO_REALTIME_PROVIDER_BAIDU` with `AUDIO_REALTIME_PROVIDER_DOUBAO`
   - map Doubao to the official realtime default WSS URL
   - keep realtime sample rate on the 24k PCM path
   - remove Baidu-only `app_id/api_secret/license/device_id/user_id` requirement from realtime validation
   - update provider error strings to `openai_compatible, qwen, doubao`
2. `configure-ui/src/types/audioConfig.ts`
   - replace the provider enum/defaults from `baidu` to `doubao`
   - remove Baidu-specific 16k sample-rate branch from realtime helpers
3. `configure-ui/src/pages/AudioConfigPanel.tsx`
   - remove all Baidu-only realtime field branches
   - add Doubao provider label and keep the generic `api_key/model/voice/ws_url/instructions` contract
4. `configure-ui/src/i18n/locales/*.ts`
   - replace Baidu realtime labels/help/error copy with Doubao wording

- [ ] **Step 6: Run the targeted tests again**

Run:

```bash
cargo test --lib doubao_realtime_validation_requires_model_voice_and_api_key
cargo test --lib realtime_provider_validation_rejects_removed_baidu_provider
cd configure-ui && npm test -- --runInBand audioConfig
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/config.rs configure-ui/src/types/audioConfig.ts configure-ui/src/pages/AudioConfigPanel.tsx configure-ui/src/i18n/locales/zh-CN.ts configure-ui/src/i18n/locales/en-US.ts
git commit -m "Switch realtime voice config and UI to Doubao"
```

### Task 2: Replace The Runtime Adapter And Remove Baidu Realtime Protocol

**Files:**
- Modify: `src/audio/realtime.rs`
- Modify: `src/audio/voice_session.rs` if provider-specific assumptions leak upward
- Test: `src/audio/realtime.rs`

- [ ] **Step 1: Write the failing runtime tests**

Add tests covering:

```rust
#[test]
fn doubao_ws_url_and_headers_follow_provider_contract() {
    let cfg = realtime_audio_cfg(AUDIO_REALTIME_PROVIDER_DOUBAO);
    let url = build_realtime_ws_url(RealtimeProvider::Doubao, &cfg).unwrap();
    let headers = build_realtime_headers(RealtimeProvider::Doubao, &cfg).unwrap();

    assert!(url.starts_with("wss://"));
    assert!(headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Authorization")));
}
```

```rust
#[test]
fn server_message_marks_session_ready_for_doubao_handshake() {
    let payload = br#"{"type":"session.created"}"#;
    assert!(server_message_marks_session_ready(RealtimeProvider::Doubao, payload));
}
```

```rust
#[test]
fn baidu_specific_runtime_paths_are_removed() {
    assert!(RealtimeProvider::parse("baidu").is_err());
}
```

- [ ] **Step 2: Run the failing runtime tests**

Run:

```bash
cargo test --lib doubao_ws_url_and_headers_follow_provider_contract
cargo test --lib server_message_marks_session_ready_for_doubao_handshake
cargo test --lib baidu_specific_runtime_paths_are_removed
```

Expected: FAIL because `audio::realtime` still contains Baidu protocol/url/license branches.

- [ ] **Step 3: Implement the minimal runtime replacement**

Implement in `src/audio/realtime.rs`:

1. replace the enum branch `Baidu` with `Doubao`
2. remove Baidu-only constants and state:
   - raw16k codec override
   - license activation bookkeeping
   - LIC handling
   - Baidu binary audio prefix stripping
   - Baidu-specific response activity inference
3. implement Doubao-specific:
   - URL construction
   - header construction
   - session-init payload
   - server-ready detection
   - downlink audio/text event handling
4. keep all existing generic runtime governance:
   - local endpoint/VAD
   - stale-turn fencing
   - voice-exclusive mode switch
   - external WSS suspend/resume
   - metrics accounting

- [ ] **Step 4: Run the targeted runtime tests**

Run:

```bash
cargo test --lib doubao_ws_url_and_headers_follow_provider_contract
cargo test --lib server_message_marks_session_ready_for_doubao_handshake
cargo test --lib baidu_specific_runtime_paths_are_removed
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/audio/realtime.rs src/audio/voice_session.rs
git commit -m "Replace Baidu realtime runtime with Doubao"
```

### Task 3: Remove Baidu Realtime Leftovers And Regressions

**Files:**
- Modify: `src/config.rs`
- Modify: `src/audio/realtime.rs`
- Modify: `configure-ui/src/types/audioConfig.ts`
- Modify: `configure-ui/src/pages/AudioConfigPanel.tsx`
- Modify: `configure-ui/src/i18n/locales/zh-CN.ts`
- Modify: `configure-ui/src/i18n/locales/en-US.ts`
- Modify: `docs/zh-cn/config-api.md`
- Modify: `docs/en-us/config-api.md`

- [ ] **Step 1: Write the failing cleanup tests**

Add/adjust regression tests to assert:

```rust
#[test]
fn realtime_required_sample_rate_is_uniform_after_baidu_removal() {
    assert_eq!(audio_realtime_required_sample_rate("doubao"), AUDIO_REALTIME_PCM16_SAMPLE_RATE);
    assert_eq!(audio_realtime_required_sample_rate("openai_compatible"), AUDIO_REALTIME_PCM16_SAMPLE_RATE);
}
```

```ts
it('normalizes saved audio config away from removed baidu realtime provider', () => {
  const normalized = normalizeAudioConfig({
    ...defaultAudioConfig(),
    realtime: { ...defaultAudioConfig().realtime, provider: 'baidu' as any },
  })
  expect(normalized.realtime.provider).toBe(DEFAULT_REALTIME_PROVIDER)
})
```

- [ ] **Step 2: Run the failing cleanup tests**

Run:

```bash
cargo test --lib realtime_required_sample_rate_is_uniform_after_baidu_removal
cd configure-ui && npm test -- --runInBand audioConfig
```

Expected: FAIL until all leftover Baidu branches are removed.

- [ ] **Step 3: Remove the leftovers**

Implement:

1. delete or rewrite Baidu-only tests in `src/audio/realtime.rs` and `src/config.rs`
2. remove Baidu-only field help/copy in configure-ui
3. update config API docs so realtime provider lists and sample-rate statements no longer mention Baidu

- [ ] **Step 4: Run the targeted cleanup tests**

Run:

```bash
cargo test --lib realtime_required_sample_rate_is_uniform_after_baidu_removal
cd configure-ui && npm test -- --runInBand audioConfig
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/audio/realtime.rs configure-ui/src/types/audioConfig.ts configure-ui/src/pages/AudioConfigPanel.tsx configure-ui/src/i18n/locales/zh-CN.ts configure-ui/src/i18n/locales/en-US.ts docs/zh-cn/config-api.md docs/en-us/config-api.md
git commit -m "Remove Baidu realtime leftovers"
```

### Task 4: Verify End-To-End Build Surfaces

**Files:**
- Verify only

- [ ] **Step 1: Run Rust formatting and full library tests**

```bash
cargo fmt --all
cargo test --lib
cargo check
```

- [ ] **Step 2: Run configure-ui verification**

```bash
cd configure-ui && npm test
cd configure-ui && npm run build
```

- [ ] **Step 3: Inspect the diff and commit any final polish**

```bash
git status --short
git diff --stat
```
