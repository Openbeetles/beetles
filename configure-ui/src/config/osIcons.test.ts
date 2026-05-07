import test from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { OS_ICON_DASHBOARD } from "./osIcons.ts";

const dashboardCardIconKeys = [
  "healthDetails",
  "governance",
  "executionTiming",
  "turnProtocol",
  "httpStorageStream",
  "voiceAudio",
  "programmableReasoning",
  "storageMedia",
] as const;

const publicRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../public",
);

function publicIconPath(src: string): string {
  assert.equal(src.startsWith("/icons/"), true, `${src} must use public icon path`);
  return path.join(publicRoot, src.slice(1));
}

test("dashboard detail cards use dedicated semantic 3D icons", () => {
  const iconSources = dashboardCardIconKeys.map((key) => OS_ICON_DASHBOARD[key]);

  assert.equal(
    new Set(iconSources).size,
    iconSources.length,
    "detail cards must not share a reused fallback icon",
  );

  for (const src of iconSources) {
    assert.equal(existsSync(publicIconPath(src)), true, `${src} must exist`);
  }

  assert.notEqual(OS_ICON_DASHBOARD.healthDetails, OS_ICON_DASHBOARD.strategy);
  assert.notEqual(OS_ICON_DASHBOARD.governance, OS_ICON_DASHBOARD.runtime);
  assert.notEqual(OS_ICON_DASHBOARD.executionTiming, OS_ICON_DASHBOARD.runtime);
  assert.notEqual(OS_ICON_DASHBOARD.turnProtocol, OS_ICON_DASHBOARD.workflow);
  assert.notEqual(OS_ICON_DASHBOARD.httpStorageStream, OS_ICON_DASHBOARD.storage);
  assert.notEqual(OS_ICON_DASHBOARD.voiceAudio, OS_ICON_DASHBOARD.runtime);
  assert.notEqual(OS_ICON_DASHBOARD.programmableReasoning, OS_ICON_DASHBOARD.workflow);
  assert.notEqual(OS_ICON_DASHBOARD.storageMedia, OS_ICON_DASHBOARD.storage);
});
