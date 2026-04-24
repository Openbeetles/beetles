import test from "node:test";
import assert from "node:assert/strict";
import {
  clearDirtyOwner,
  clearDirtyOwners,
  isDirtyOwners,
  setDirtyOwner,
  type DirtyOwners,
} from "./unsavedDirtyModel.ts";

test("dirty owners keep independent unsaved state", () => {
  let owners: DirtyOwners = new Set();
  owners = setDirtyOwner(owners, "ai-config", true);
  owners = setDirtyOwner(owners, "device-access", true);

  assert.equal(isDirtyOwners(owners), true);

  owners = setDirtyOwner(owners, "ai-config", false);

  assert.equal(owners.has("ai-config"), false);
  assert.equal(owners.has("device-access"), true);
  assert.equal(isDirtyOwners(owners), true);
});

test("dirty owner cleanup removes only the matching owner unless clearing all", () => {
  let owners: DirtyOwners = new Set(["system-config", "device-access"]);

  owners = clearDirtyOwner(owners, "system-config");
  assert.deepEqual([...owners], ["device-access"]);

  owners = clearDirtyOwners();
  assert.equal(isDirtyOwners(owners), false);
});
