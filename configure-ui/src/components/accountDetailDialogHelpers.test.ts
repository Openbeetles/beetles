import assert from "node:assert/strict";
import test from "node:test";
import type { TFunction } from "i18next";
import { formatProbeMessage } from "./accountDetailDialogHelpers.ts";

const t = ((key: string) => {
  switch (key) {
    case "accounts.probeDisposition.ready":
      return "Ready";
    case "accounts.probeDisposition.missing_credential":
      return "Missing credential";
    case "accounts.probeDisposition.unsupported":
      return "Unsupported";
    case "accounts.probeReason.credential_missing":
      return "Credential not configured";
    case "accounts.probeReason.probe_adapter_unavailable":
      return "Probe adapter unavailable";
    case "accounts.probeReason.imap_ok":
      return "IMAP probe succeeded";
    default:
      return key;
  }
}) as TFunction<"translation", undefined>;

test("formatProbeMessage localizes known disposition and reason keys", () => {
  assert.equal(
    formatProbeMessage("missing_credential", "credential_missing", t),
    "Missing credential · Credential not configured",
  );
});

test("formatProbeMessage falls back to raw reason when no translation key exists", () => {
  assert.equal(
    formatProbeMessage("unsupported", "provider_timeout", t),
    "Unsupported · provider_timeout",
  );
});

test("formatProbeMessage omits empty reason text", () => {
  assert.equal(formatProbeMessage("ready", "", t), "Ready");
});
