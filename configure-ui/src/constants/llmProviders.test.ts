import assert from "node:assert/strict";
import test from "node:test";
import {
  apiUrlAfterProviderChange,
  defaultApiUrlForProvider,
  defaultModelForProvider,
  modelAfterProviderChange,
} from "./llmProviders.ts";

test("provider change resets model to the new provider default", () => {
  assert.equal(
    modelAfterProviderChange("custom-a-model", "openai", "deepseek"),
    defaultModelForProvider("deepseek"),
  );
});

test("provider change keeps custom api url only when explicitly customized", () => {
  assert.equal(
    apiUrlAfterProviderChange(
      defaultApiUrlForProvider("openai"),
      "openai",
      "deepseek",
    ),
    defaultApiUrlForProvider("deepseek"),
  );
  assert.equal(
    apiUrlAfterProviderChange("https://proxy.example.test/v1", "openai", "deepseek"),
    "https://proxy.example.test/v1",
  );
});
