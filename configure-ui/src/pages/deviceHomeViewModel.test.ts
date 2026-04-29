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
  buildFaultAndRecoveryMetrics,
  buildMemoryMetrics,
  buildRuntimeTelemetryFields,
  buildRuntimeStrategyView,
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
    errors_agent_router: 1,
    errors_llm_request: 2,
    errors_channel_dispatch: 3,
    wifi_reconnect_total: 3,
    wifi_ap_restart_total: 4,
  };

  const grouped = buildFaultAndRecoveryMetrics(metrics);

  assert.deepEqual(
    grouped.faults.map((item) => item.id),
    ["errors_agent_router", "errors_llm_request", "errors_channel_dispatch"],
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
