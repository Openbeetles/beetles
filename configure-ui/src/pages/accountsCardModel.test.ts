import assert from "node:assert/strict";
import test from "node:test";
import type { AccountSummary } from "../types/accountConfig.ts";
import { buildAccountCardModel } from "./accountsCardModel.ts";

function t(key: string) {
  return key;
}

const baseAccount: AccountSummary = {
  account_key: "imap-smtp-other-657778650-qq-com",
  provider_kind: "imap_smtp",
  account_label: "657778650@qq.com",
  identity_class: "other",
  enabled_capabilities: ["mail"],
  selected_for_capabilities: ["mail"],
  readiness: "needs_configuration",
  next_action: "configure_account",
  missing_fields_count: 2,
  has_runtime_error: true,
};

test("buildAccountCardModel demotes provider/raw ids while keeping the readable title primary", () => {
  const model = buildAccountCardModel(baseAccount, {
    t,
    providerLabel: "通用邮箱",
  });

  assert.equal(model.eyebrow, "通用邮箱");
  assert.equal(model.title, "657778650@qq.com");
  assert.equal(model.identityLabel, "accounts.identity.other");
  assert.deepEqual(model.capabilityLabels, ["accounts.capabilityShort.mail"]);
  assert.equal(model.providerMeta, "imap_smtp");
  assert.equal(model.rawKeyMeta, "imap-smtp-other-657778650-qq-com");
  assert.equal(model.showRuntimeFlag, true);
});

test("buildAccountCardModel avoids duplicating the raw key in the footer when it is already the title", () => {
  const model = buildAccountCardModel(
    {
      ...baseAccount,
      account_label: "   ",
      account_key: "plain-key",
      provider_kind: "google_calendar",
      enabled_capabilities: ["calendar"],
      selected_for_capabilities: [],
      next_action: "none",
      has_runtime_error: false,
      readiness: "ready",
    },
    {
      t,
      providerLabel: "Google Calendar",
    },
  );

  assert.equal(model.title, "plain-key");
  assert.equal(model.providerMeta, "google_calendar");
  assert.equal(model.rawKeyMeta, "");
  assert.equal(model.showRuntimeFlag, false);
});
