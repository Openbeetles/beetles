import assert from "node:assert/strict";
import test from "node:test";
import { defaultAudioConfig } from "./audioConfig.ts";

test("default realtime instructions use the product-facing Beetle OS name", () => {
  const config = defaultAudioConfig();

  assert.match(config.realtime.instructions, /Beetle OS/);
  assert.doesNotMatch(config.realtime.instructions, /Beetles OS/);
});
