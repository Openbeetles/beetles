import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const source = readFileSync(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "AudioConfigPanel.tsx"),
  "utf8",
);

test("audio config tabs do not add extra top padding before the active panel", () => {
  assert.equal(
    /<ConfigPanelTabs[\s\S]*?\/>\s*<Box>\s*\{activeAudioTab/.test(source),
    true,
    "tab panel content should start immediately after ConfigPanelTabs",
  );
  assert.equal(
    /<Box\s+sx=\{\{\s*pt:/.test(source),
    false,
    "audio tab content must not add a second top-padding layer",
  );
});
