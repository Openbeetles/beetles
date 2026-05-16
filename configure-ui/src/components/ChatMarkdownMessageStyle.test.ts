import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const cssPath = join(dirname(fileURLToPath(import.meta.url)), "ChatMarkdownMessage.css");
const css = readFileSync(cssPath, "utf8");

test("chat markdown messages preserve authored line feeds with relaxed leading", () => {
  assert.match(
    css,
    /\.chat-markdown-message\s*{[^}]*white-space:\s*pre-wrap;/s,
  );
  assert.match(css, /\.chat-markdown-message\s*{[^}]*line-height:\s*1\.72;/s);
});
