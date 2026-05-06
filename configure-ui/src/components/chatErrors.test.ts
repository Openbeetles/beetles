import assert from "node:assert/strict";
import test from "node:test";
import { translateChatApiError } from "./chatErrors.ts";

function t(key: string, options?: { defaultValue?: string }): string {
  const translations: Record<string, string> = {
    "chat.loadFailed": "无法加载聊天记录",
    "chat.resource_pressure": "设备当前资源不足，暂时无法处理聊天请求，请稍后再试。",
    "chat.content_required": "请输入消息后再发送。",
    "http.route_worker_memory_low": "设备当前内存余量不足，暂时无法启动该配置任务。",
  };
  return translations[key] ?? options?.defaultValue ?? key;
}

test("translateChatApiError maps route worker memory pressure to chat wording", () => {
  assert.equal(
    translateChatApiError(t, "http.route_worker_memory_low", "chat.loadFailed"),
    "设备当前资源不足，暂时无法处理聊天请求，请稍后再试。",
  );
});

test("translateChatApiError keeps existing chat keys and prose errors", () => {
  assert.equal(
    translateChatApiError(t, "chat.content_required", "chat.loadFailed"),
    "请输入消息后再发送。",
  );
  assert.equal(
    translateChatApiError(t, "upstream unavailable", "chat.loadFailed"),
    "upstream unavailable",
  );
});
