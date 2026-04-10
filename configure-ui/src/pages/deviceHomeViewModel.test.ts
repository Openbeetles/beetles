import test from "node:test";
import assert from "node:assert/strict";
import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
  SystemInfoData,
} from "../api/endpoints/system.ts";
import {
  buildDeviceSummaryFields,
  buildFaultAndRecoveryMetrics,
  buildMemoryMetrics,
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
    system_status: "running",
    current_time: "2026-04-10 12:00:00 UTC",
    firmware_version: "0.1.0",
    board_id: "linux",
    hardware_model: "Orange Pi Zero LTS",
    lan_ip: "192.168.1.37",
    ota_available: false,
    locale: "zh-CN",
  };
  const health: HealthData = {
    audio: {
      duplex_profile: "speaker_only",
    },
  };

  const fields = buildDeviceSummaryFields(systemInfo, health);

  assert.deepEqual(
    fields.map((item) => item.id),
    [
      "board_id",
      "hardware_model",
      "lan_ip",
      "firmware_version",
      "system_status",
      "audio_duplex_profile",
      "locale",
      "ota_available",
      "current_time",
    ],
  );
});

test("buildFaultAndRecoveryMetrics keeps WiFi recovery events out of fault counters", () => {
  const metrics: MetricsSnapshotData = {
    errors_agent_router: 1,
    errors_llm_request: 2,
    wifi_reconnect_total: 3,
    wifi_ap_restart_total: 4,
  };

  const grouped = buildFaultAndRecoveryMetrics(metrics);

  assert.deepEqual(
    grouped.faults.map((item) => item.id),
    ["errors_agent_router", "errors_llm_request"],
  );
  assert.deepEqual(
    grouped.recovery.map((item) => item.id),
    ["wifi_reconnect_total", "wifi_ap_restart_total"],
  );
});
