import test from "node:test";
import assert from "node:assert/strict";
import { buildSystemLogMetricItems } from "./systemLogsPageModel.ts";
import type { MetricsSnapshotData } from "../api/endpoints/system.ts";

test("buildSystemLogMetricItems keeps a bounded operator-facing metric set", () => {
  const metrics: MetricsSnapshotData & Record<string, unknown> = {
    messages_in: 10,
    system_messages_in: 7,
    llm_calls: 3,
    tool_errors: 1,
    internal_debug_counter_that_should_not_render: 999,
  };

  const items = buildSystemLogMetricItems(metrics);

  assert.deepEqual(
    items.map((item) => item.id),
    ["messages_in", "system_messages_in", "llm_calls", "tool_errors"],
  );
  assert.equal(
    items
      .map((item) => String(item.id))
      .includes("internal_debug_counter_that_should_not_render"),
    false,
  );
});

test("buildSystemLogMetricItems keeps total and user inbound metrics distinct", () => {
  const metrics: MetricsSnapshotData = {
    messages_in: 10,
    user_messages_in: 4,
  };

  const items = buildSystemLogMetricItems(metrics);

  assert.deepEqual(
    items.map((item) => [item.id, item.labelKey]),
    [
      ["messages_in", "device.systemStatusTotalMessagesIn"],
      ["user_messages_in", "device.systemStatusMessagesIn"],
    ],
  );
});
