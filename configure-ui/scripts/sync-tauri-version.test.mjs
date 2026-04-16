import test from "node:test";
import assert from "node:assert/strict";

import { syncVersionArtifacts } from "./sync-tauri-version.mjs";

test("syncVersionArtifacts updates tauri.conf.json and Cargo.toml to package version", () => {
  const result = syncVersionArtifacts({
    packageJsonText: JSON.stringify({
      name: "beetle-configure-ui",
      version: "1.2.3",
    }),
    tauriConfText: JSON.stringify({
      productName: "Beetle Configure UI",
      version: "0.0.0",
    }),
    cargoTomlText: `[package]
name = "beetle-configure-ui-desktop"
version = "0.0.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = [] }
`,
  });

  assert.equal(result.version, "1.2.3");
  assert.equal(JSON.parse(result.nextTauriConfText).version, "1.2.3");
  assert.match(result.nextCargoTomlText, /version = "1.2.3"/);
});

test("syncVersionArtifacts only replaces version inside package section", () => {
  const result = syncVersionArtifacts({
    packageJsonText: JSON.stringify({
      name: "beetle-configure-ui",
      version: "2.0.0",
    }),
    tauriConfText: JSON.stringify({
      productName: "Beetle Configure UI",
      version: "0.0.0",
    }),
    cargoTomlText: `[package]
name = "beetle-configure-ui-desktop"
version = "0.0.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = [] }
`,
  });

  const packageVersionMatches = result.nextCargoTomlText.match(/version = "2.0.0"/g) ?? [];
  assert.equal(packageVersionMatches.length, 1);
  assert.match(result.nextCargoTomlText, /tauri = \{ version = "2", features = \[\] \}/);
});
