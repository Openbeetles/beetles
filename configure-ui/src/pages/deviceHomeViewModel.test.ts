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
    ota_available: false,
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
      "ota_available",
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
    wifi: "connected",
    last_error: "none",
  };
  const resource: ResourceSnapshotData = {
    pressure: "Critical",
  };

  const key = buildDeviceOperationalStatusKey(health, resource);

  assert.equal(key, "device.runtimeSummaryCritical");
});
