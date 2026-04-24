import test from "node:test";
import assert from "node:assert/strict";
import type { AccountAssessment, AccountFieldState } from "../types/accountConfig.ts";
import { buildAccountDetailSummaryModel } from "./accountDetailSummaryModel.ts";

const baseAssessment: AccountAssessment = {
  account_key: "acct-1",
  provider_kind: "imap_smtp",
  enabled_capabilities: ["mail"],
  credential_present: true,
  credential_configured: true,
  probe_supported: true,
  missing_fields: [],
  missing_field_details: [],
  readiness: "ready",
  next_action: "none",
};

const fields: AccountFieldState[] = [
  {
    key: "access_token",
    label_key: "providerFields.accessToken",
    description_key: "",
    location: "access_token",
    value_kind: "secret",
    required: true,
    secret: true,
    configured: true,
  },
  {
    key: "imap_host",
    label_key: "providerFields.imapHost",
    description_key: "",
    location: "metadata",
    value_kind: "hostname",
    required: true,
    secret: false,
    configured: true,
  },
];

function labelFor(field: AccountFieldState): string {
  return field.key === "imap_host" ? "IMAP 主机" : field.key;
}

test("account detail summary hides no-op next action status", () => {
  const model = buildAccountDetailSummaryModel({
    assessment: baseAssessment,
    fields,
    labelFor,
    probeMessage: null,
  });

  assert.deepEqual(model.statusItems, [
    { key: "readiness", labelKey: "accounts.readiness.ready", tone: "success" },
  ]);
  assert.equal(model.missingFieldsText, null);
  assert.equal(model.probeMessage, null);
});

test("account detail summary keeps missing field copy inline", () => {
  const model = buildAccountDetailSummaryModel({
    assessment: {
      ...baseAssessment,
      readiness: "needs_configuration",
      next_action: "configure_account",
      missing_fields: ["imap_host", "unknown_field"],
    },
    fields,
    labelFor,
    probeMessage: "IMAP 探测成功",
  });

  assert.deepEqual(model.statusItems, [
    {
      key: "readiness",
      labelKey: "accounts.readiness.needs_configuration",
      tone: "warning",
    },
    {
      key: "nextAction",
      labelKey: "accounts.nextAction.configure_account",
      tone: "default",
    },
  ]);
  assert.equal(model.missingFieldsText, "IMAP 主机, unknown_field");
  assert.equal(model.probeMessage, "IMAP 探测成功");
});
