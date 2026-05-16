import assert from "node:assert/strict";
import test from "node:test";
import {
  appendMarkdownStreamDelta,
  hasVisibleMarkdownText,
  isSafeMarkdownUrl,
  normalizeMarkdownText,
  parseChatMarkdown,
} from "./ChatMarkdownMessageModel.ts";

test("normalizeMarkdownText keeps streamed markdown line endings stable", () => {
  assert.equal(normalizeMarkdownText("a\r\nb\rc"), "a\nb\nc");
});

test("appendMarkdownStreamDelta preserves raw stream boundaries before rendering", () => {
  const first = appendMarkdownStreamDelta("", "a\r");
  assert.equal(first.raw, "a\r");
  assert.equal(first.rendered, "a\n");

  const second = appendMarkdownStreamDelta(first.raw, "\nb");
  assert.equal(second.raw, "a\r\nb");
  assert.equal(second.rendered, "a\nb");
});

test("hasVisibleMarkdownText keeps blank streaming deltas in the waiting state", () => {
  assert.equal(hasVisibleMarkdownText(""), false);
  assert.equal(hasVisibleMarkdownText("\r\n  \n"), false);
  assert.equal(hasVisibleMarkdownText("  **OK**  "), true);
});

test("parseChatMarkdown supports assistant status tables and emphasis", () => {
  const tree = parseChatMarkdown([
    "系统状态总览",
    "",
    "| 项目 | 状态 |",
    "| --- | --- |",
    "| 芯片 | **ESP32-S3** |",
  ].join("\n"));

  assert.equal(tree.children[0]?.type, "paragraph");
  assert.equal(tree.children[1]?.type, "table");
  if (tree.children[1]?.type !== "table") {
    throw new Error("expected table node");
  }
  assert.equal(tree.children[1].children.length, 2);
  assert.equal(tree.children[1].children[1]?.children[1]?.children[0]?.type, "strong");
});

test("parseChatMarkdown does not repair collapsed pipe tables", () => {
  const tree = parseChatMarkdown(
    "系统状态总览 | 项目 | 状态 |------|------| 芯片 | ESP32-S3 | WiFi | 已连接",
  );

  assert.equal(tree.children.length, 1);
  assert.equal(tree.children[0]?.type, "paragraph");
});

test("isSafeMarkdownUrl rejects script-like links", () => {
  assert.equal(isSafeMarkdownUrl("https://example.com"), true);
  assert.equal(isSafeMarkdownUrl("/local/path"), true);
  assert.equal(isSafeMarkdownUrl("#section"), true);
  assert.equal(isSafeMarkdownUrl("//example.com"), false);
  assert.equal(isSafeMarkdownUrl("/\\example.com"), false);
  assert.equal(isSafeMarkdownUrl("javascript:alert(1)"), false);
  assert.equal(isSafeMarkdownUrl("data:text/html,boom"), false);
});
