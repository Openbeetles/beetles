import test from "node:test";
import assert from "node:assert/strict";
import { createLatestRequestGuard } from "./latestRequest.ts";

test("latest request guard accepts only the newest issued request", () => {
  const guard = createLatestRequestGuard();
  const first = guard.next();
  const second = guard.next();

  assert.equal(guard.isCurrent(first), false);
  assert.equal(guard.isCurrent(second), true);
});

test("latest request guard can invalidate in-flight work without issuing a replacement", () => {
  const guard = createLatestRequestGuard();
  const request = guard.next();

  guard.invalidate();

  assert.equal(guard.isCurrent(request), false);
});
