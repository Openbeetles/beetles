import test from "node:test";
import assert from "node:assert/strict";
import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
  SystemInfoData,
} from "../api/endpoints/system.ts";
import {
  buildDeviceOperationalStatusKey,
  buildDeviceSummaryFields,
  buildExecutionTimingFields,
  buildFaultAndRecoveryMetrics,
  buildHealthDetailFields,
  buildHttpStorageStreamFields,
  buildMemoryMetrics,
  buildProgrammableReasoningFields,
  buildResourceGovernanceFields,
  buildResourceRiskFields,
  buildRuntimeTelemetryFields,
  buildRuntimeStrategyView,
  buildStorageMediaDetailFields,
  buildTurnProtocolFields,
  buildVoiceAudioTelemetryFields,
} from "./deviceHomeViewModel.ts";

test("buildMemoryMetrics hides Linux-only non-applicable PSRAM and largest-block placeholders", () => {
  const resource: ResourceSnapshotData = {
    heap_free_internal: 1024,
    heap_free_spiram: 0,
    heap_largest_block_internal: 0,
  };

  const metrics = buildMemoryMetrics("linux", resource);

  assert.deepEqual(
    metrics.map((item) => item.id),
    ["heap_internal"],
  );
});

test("buildMemoryMetrics shows ESP PSRAM free used total min and largest block from resource", () => {
  const resource: ResourceSnapshotData = {
    heap_free_internal: 128 * 1024,
    heap_min_free_internal: 96 * 1024,
    heap_free_spiram: 7 * 1024 * 1024,
    heap_used_spiram_est: 512 * 1024,
    heap_total_spiram: 8 * 1024 * 1024,
    heap_min_free_spiram: 6 * 1024 * 1024,
    heap_largest_block_spiram: 5 * 1024 * 1024,
    heap_largest_block_internal: 64 * 1024,
  };

  const metrics = buildMemoryMetrics("esp", resource);

  assert.deepEqual(
    metrics.map((item) => item.id),
    [
      "heap_internal",
      "heap_internal_min",
      "heap_spiram_free",
      "heap_spiram_used_est",
      "heap_spiram_total",
      "heap_spiram_min",
      "heap_spiram_largest",
      "heap_largest",
    ],
  );
});

test("buildDeviceSummaryFields includes the full homepage device summary fields", () => {
  const systemInfo: SystemInfoData = {
    product_name: "beetle",
    current_time: "2026-04-10 12:00:00 UTC",
    firmware_version: "0.1.0",
    board_id: "linux",
    hardware_model: "Orange Pi Zero LTS",
    lan_ip: "192.168.1.37",
    locale: "zh-CN",
    os_type: "Linux",
    kernel_version: "5.4.27-sunxi",
    cpu_model: "ARMv7 Processor rev 5",
    cpu_cores: 4,
    storage_media: [
      {
        id: "rootfs",
        kind: "emmc",
        label: "eMMC",
        present: true,
        mounted: true,
        mount_path: "/",
        filesystem: "ext4",
        source: "/dev/mmcblk0p2",
        removable: false,
        is_system_root: true,
        is_state_root: true,
        capacity_bytes: 31_000_000_000,
        free_bytes: 28_000_000_000,
      },
    ],
  };
  const health: HealthData = {
    audio: {
      duplex_profile: "speaker_only",
    },
  };
  const derivedStatusKey = "device.runtimeSummaryHealthy";

  const fields = buildDeviceSummaryFields(systemInfo, health, derivedStatusKey);

  assert.deepEqual(
    fields.map((item) => item.id),
    [
      "product_name",
      "board_id",
      "hardware_model",
      "os_type",
      "kernel_version",
      "cpu_model",
      "cpu_cores",
      "lan_ip",
      "storage_media",
      "firmware_version",
      "system_status",
      "audio_duplex_profile",
      "locale",
      "current_time",
    ],
  );
  assert.equal(
    fields.find((item) => item.id === "system_status")?.value,
    derivedStatusKey,
  );
  assert.match(
    String(fields.find((item) => item.id === "storage_media")?.value),
    /eMMC/,
  );
});

test("buildFaultAndRecoveryMetrics keeps WiFi recovery events out of fault counters", () => {
  const metrics: MetricsSnapshotData = {
    errors_agent_chat: 1,
    errors_llm_request: 2,
    errors_channel_dispatch: 3,
    wifi_reconnect_total: 3,
    wifi_ap_restart_total: 4,
  };

  const grouped = buildFaultAndRecoveryMetrics(metrics);

  assert.deepEqual(
    grouped.faults.map((item) => item.id),
    ["errors_agent_chat", "errors_llm_request", "errors_channel_dispatch"],
  );
  assert.deepEqual(
    grouped.recovery.map((item) => item.id),
    ["wifi_reconnect_total", "wifi_ap_restart_total"],
  );
});

test("buildRuntimeStrategyView explains cautious pressure with behavior hints and budget stats", () => {
  const resource: ResourceSnapshotData = {
    pressure: "Cautious",
    budget: {
      level: "Cautious",
      system_prompt_max: 4096,
      messages_max: 12 * 1024,
      response_body_max: 256 * 1024,
      reconnect_backoff_secs: 15,
    },
  };

  const strategy = buildRuntimeStrategyView(resource);

  assert.ok(strategy);
  assert.equal(strategy.headlineKey, "device.systemStatusStrategyCautious");
  assert.equal(strategy.intensity, 2);
  assert.deepEqual(strategy.behaviorKeys, [
    "device.systemStatusStrategyBehaviorCautiousReplies",
    "device.systemStatusStrategyBehaviorCautiousTools",
    "device.systemStatusStrategyBehaviorCautiousReconnect",
  ]);
  assert.deepEqual(
    strategy.budgetFields.map((item) => item.id),
    [
      "system_prompt_max",
      "messages_max",
      "response_body_max",
      "reconnect_backoff_secs",
    ],
  );
});

test("buildDeviceOperationalStatusKey derives homepage runtime state from health and resource", () => {
  const health: HealthData = {
    network_status: { sta_connected: true },
    last_error: "none",
  };
  const resource: ResourceSnapshotData = {
    pressure: "Critical",
  };

  const key = buildDeviceOperationalStatusKey(health, resource);

  assert.equal(key, "device.runtimeSummaryCritical");
});

test("buildDeviceOperationalStatusKey derives disconnected state from health network_status", () => {
  const health: HealthData = {
    network_status: { sta_connected: false },
    last_error: "none",
  };

  const key = buildDeviceOperationalStatusKey(health, { pressure: "Normal" });

  assert.equal(key, "device.runtimeSummaryWifiDisconnected");
});

test("buildRuntimeTelemetryFields hides Linux-only runtime metrics on ESP", () => {
  const resource: ResourceSnapshotData = {
    active_http_count: 1,
    active_wss_count: 2,
    active_agent_tasks: 0,
    session_count: 3,
    inbound_depth: 4,
    outbound_depth: 5,
  };
  const metrics: MetricsSnapshotData = {
    messages_in: 10,
    agent_messages_in: 16,
    system_messages_in: 6,
    messages_out: 11,
    llm_calls: 12,
    llm_last_ms: 13,
    tool_calls: 14,
    dispatch_send_ok: 15,
    last_active_epoch_secs: 1_775_792_000,
  };

  const fields = buildRuntimeTelemetryFields("esp", resource, metrics);

  assert.deepEqual(
    fields.map((item) => item.id),
    [
      "active_http_count",
      "active_wss_count",
      "active_agent_tasks",
      "session_count",
      "messages_in",
      "agent_messages_in",
      "system_messages_in",
      "messages_out",
      "llm_calls",
      "llm_last_ms",
      "tool_calls",
      "dispatch_send_ok",
      "inbound_depth",
      "outbound_depth",
      "last_active_epoch_secs",
    ],
  );
});

test("buildRuntimeTelemetryFields prefers explicit user inbound metric alias", () => {
  const fields = buildRuntimeTelemetryFields(
    "esp",
    null,
    {
      messages_in: 99,
      user_messages_in: 10,
    },
  );

  assert.equal(fields.find((item) => item.id === "messages_in")?.value, 10);
});

test("buildRuntimeTelemetryFields keeps Linux-only runtime metrics on Linux", () => {
  const resource: ResourceSnapshotData = {
    active_http_count: 1,
    active_wss_count: 2,
    active_agent_tasks: 0,
    session_count: 3,
    inbound_depth: 4,
    outbound_depth: 5,
    cpu_usage_percent: 7.5,
    process_memory_kb: 8192,
    load_average: [0.1, 0.2, 0.3],
  };
  const metrics: MetricsSnapshotData = {
    messages_in: 10,
    agent_messages_in: 16,
    system_messages_in: 6,
    messages_out: 11,
    llm_calls: 12,
    llm_last_ms: 13,
    tool_calls: 14,
    dispatch_send_ok: 15,
    last_active_epoch_secs: 1_775_792_000,
  };

  const fields = buildRuntimeTelemetryFields("linux", resource, metrics);

  assert.deepEqual(
    fields.map((item) => item.id),
    [
      "active_http_count",
      "active_wss_count",
      "active_agent_tasks",
      "session_count",
      "messages_in",
      "agent_messages_in",
      "system_messages_in",
      "messages_out",
      "llm_calls",
      "llm_last_ms",
      "tool_calls",
      "dispatch_send_ok",
      "inbound_depth",
      "outbound_depth",
      "last_active_epoch_secs",
      "cpu_usage_percent",
      "process_memory_kb",
      "load_average",
    ],
  );
});

test("homepage deep detail builders expose health resource metrics and system_info fields", () => {
  const health: HealthData = {
    status: "degraded",
    network_status: {
      stage: "sta_connected",
      sta_connected: true,
      wall_clock_trusted: false,
    },
  };
  const resource: ResourceSnapshotData = {
    tls_fragmentation_risk: "healthy",
    storage_contention_risk: "Critical",
    governance_metrics: {
      runtime_spawn_failure_total: 1,
      http_route_reject_total: 2,
      inbound_queue_full_total: 3,
      inbound_defer_total: 4,
      inbound_drop_total: 5,
      event_ingress_enqueued_total: 6,
      event_ingress_rejected_total: 7,
      event_ingress_purged_total: 8,
      event_ingress_cancelled_total: 9,
      event_ingress_stale_drop_total: 10,
    },
  };
  const metrics: MetricsSnapshotData = {
    llm_request_body_last_bytes: 100,
    llm_request_body_max_bytes: 200,
    request_semantics_last_ms: 3,
    tool_exec_last_ms: 4,
    mental_privacy_review_last_ms: 5,
    ttft_last_ms: 6,
    e2e_last_ms: 7,
    post_reply_last_ms: 8,
    user_queue_wait_last_ms: 9,
    system_queue_wait_last_ms: 10,
    cron_e2e_last_ms: 11,
    react_rounds_last: 12,
    tool_calls_last: 13,
    tool_protocol_forced_rounds: 17,
    tool_protocol_violation: 18,
    final_answer_calls: 19,
    outbound_enqueue_fail: 20,
    tool_succeeded_final_drift_total: 21,
    empty_final_blocked_total: 22,
    internal_error_copy_suppressed_total: 23,
    channel_http_ok: 24,
    channel_http_fail: 25,
    http_permit_wait_last_ms: 26,
    http_route_queue_wait_last_ms: 27,
    http_route_handler_last_ms: 28,
    http_route_timeout_total: 29,
    voice_input_last_ms: 30,
    voice_output_last_ms: 33,
    voice_input_fail_total: 34,
    voice_output_fail_total: 35,
    voice_interrupt_total: 37,
    voice_interrupt_missed_total: 1,
    voice_no_speech_timeout_total: 41,
    voice_response_wait_timeout_total: 42,
    voice_playback_timeout_total: 43,
    wake_trigger_total: 44,
    storage_lock_ops_total: 61,
    storage_lock_contention_total: 62,
    storage_lock_wait_last_us: 63,
    storage_lock_hold_last_us: 65,
    storage_lock_hold_last_stage: "session_append",
    errors_tls_admission: 68,
    stream_http_reuse_hits: 69,
    stream_http_creates: 70,
    stream_http_resets: 71,
    stream_http_invalidates: 72,
  };
  const systemInfo: SystemInfoData = {
    product_name: "beetle",
    firmware_version: "0.1.0",
    programmable_reasoning: {
      stage: "capability_atoms_exchange",
      execution_enabled: true,
      backend: "host_only",
      linux_only: true,
      proposal_only_persistence: false,
      product_headline: "Programmable reasoning",
      demo_scenario_count: 2,
      inspection_ready: true,
      replay_ready: false,
    },
    storage_media: [
      {
        id: "state",
        kind: "state",
        label: "State storage",
        present: true,
        mounted: true,
        mount_path: "/state",
        filesystem: "ext4",
        source: "/dev/mmcblk0p2",
        removable: false,
        is_system_root: false,
        is_state_root: true,
        capacity_bytes: 1024,
        free_bytes: 512,
      },
    ],
  };

  assert.deepEqual(buildHealthDetailFields(health).map((item) => item.id), [
    "health_status",
    "network_stage",
    "wall_clock_trusted",
  ]);
  assert.deepEqual(buildResourceRiskFields(resource).map((item) => item.id), [
    "tls_fragmentation_risk",
    "storage_contention_risk",
  ]);
  assert.equal(
    buildResourceRiskFields(resource)[0]?.value,
    "device.systemStatusRiskHealthy",
  );
  assert.equal(buildResourceGovernanceFields(resource).length, 10);
  assert.equal(buildExecutionTimingFields(metrics).length, 11);
  assert.equal(buildTurnProtocolFields(metrics).length, 12);
  assert.equal(buildHttpStorageStreamFields(metrics).length, 13);
  assert.deepEqual(buildVoiceAudioTelemetryFields(metrics).map((item) => item.id), [
    "voice_input_last_ms",
    "voice_output_last_ms",
    "wake_trigger_total",
    "voice_interrupt_total",
    "voice_interrupt_missed_total",
    "voice_input_fail_total",
    "voice_output_fail_total",
    "voice_no_speech_timeout_total",
    "voice_response_wait_timeout_total",
    "voice_playback_timeout_total",
  ]);
  assert.equal(buildProgrammableReasoningFields(systemInfo).length, 9);
  assert.equal(buildStorageMediaDetailFields(systemInfo)[0]?.fields.length, 12);
});
