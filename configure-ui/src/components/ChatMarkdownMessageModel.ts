import { fromMarkdown } from "mdast-util-from-markdown";
import { gfmStrikethroughFromMarkdown } from "mdast-util-gfm-strikethrough";
import { gfmTableFromMarkdown } from "mdast-util-gfm-table";
import { gfmTaskListItemFromMarkdown } from "mdast-util-gfm-task-list-item";
import { gfmStrikethrough } from "micromark-extension-gfm-strikethrough";
import { gfmTable } from "micromark-extension-gfm-table";
import { gfmTaskListItem } from "micromark-extension-gfm-task-list-item";
import type { Root } from "mdast";

const SAFE_LINK_PROTOCOLS = new Set(["http:", "https:", "mailto:", "tel:"]);

export function normalizeMarkdownText(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

export function appendMarkdownStreamDelta(
  accumulatedRaw: string,
  delta: string,
): { raw: string; rendered: string } {
  const raw = `${accumulatedRaw}${delta}`;
  return {
    raw,
    rendered: normalizeMarkdownText(raw),
  };
}

export function hasVisibleMarkdownText(text: string): boolean {
  return normalizeMarkdownText(text).trim().length > 0;
}

export function parseChatMarkdown(markdown: string): Root {
  return fromMarkdown(normalizeMarkdownText(markdown), {
    extensions: [gfmTable(), gfmStrikethrough(), gfmTaskListItem()],
    mdastExtensions: [
      gfmTableFromMarkdown(),
      gfmStrikethroughFromMarkdown(),
      gfmTaskListItemFromMarkdown(),
    ],
  });
}

export function isSafeMarkdownUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;
  if (trimmed.startsWith("#")) return true;
  if (trimmed.startsWith("/")) {
    return !trimmed.startsWith("//") && !trimmed.startsWith("/\\");
  }
  try {
    return SAFE_LINK_PROTOCOLS.has(new URL(trimmed).protocol);
  } catch {
    return false;
  }
}
