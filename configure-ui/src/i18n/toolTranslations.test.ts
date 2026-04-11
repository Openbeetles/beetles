import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { zhCN } from './locales/zh-CN.ts'
import { enUS } from './locales/en-US.ts'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const registryPath = path.resolve(__dirname, '../../../src/tools/registry.rs')

const TOOL_CLASS_TO_NAME: Record<string, string> = {
  GetTime: 'get_time',
  Env: 'env',
  Message: 'message',
  Task: 'task',
  Calendar: 'calendar',
  Files: 'files',
  FileEdit: 'file_edit',
  DocumentSearch: 'document_search',
  DocumentRead: 'document_read',
  DocumentExtract: 'document_extract',
  WebSearch: 'web_search',
  WebFetch: 'web_fetch',
  PdfRead: 'pdf_read',
  AnalyzeImage: 'analyze_image',
  RemindAt: 'remind_at',
  RemindList: 'remind_list',
  BoardInfo: 'board_info',
  KvStore: 'kv_store',
  PrivateGarden: 'private_garden',
  FactualMemory: 'factual_memory',
  MemorySearch: 'memory_search',
  MemoryGet: 'memory_get',
  ContinuitySnapshot: 'continuity_snapshot',
  DeviceControl: 'device_control',
  MemoryManage: 'memory_manage',
  HttpRequest: 'http_request',
  SessionManage: 'session_manage',
  FileWrite: 'file_write',
  SystemControl: 'system_control',
  CronManage: 'cron_manage',
  ProxyConfig: 'proxy_config',
  ModelConfig: 'model_config',
  NetworkScan: 'network_scan',
  SensorWatch: 'sensor_watch',
  I2cDevice: 'i2c_device',
  I2cSensor: 'i2c_sensor',
  VoiceInput: 'voice_input',
  VoiceOutput: 'voice_output',
  Shell: 'shell',
  Process: 'process',
  Network: 'network',
}

function registeredToolNames(): string[] {
  const source = fs.readFileSync(registryPath, 'utf8')
  const out = new Set<string>()
  const re = /register\(Box::new\(super::([A-Za-z0-9_]+)Tool/g
  for (const match of source.matchAll(re)) {
    const key = TOOL_CLASS_TO_NAME[match[1]]
    if (key) out.add(key)
  }
  return [...out].sort()
}

function missingToolKeys(localeTools: Record<string, string>): string[] {
  return registeredToolNames().filter((name) => !(name in localeTools))
}

test('zh-CN covers every registered production tool key', () => {
  assert.deepEqual(missingToolKeys(zhCN.translation.tools), [])
})

test('en-US covers every registered production tool key', () => {
  assert.deepEqual(missingToolKeys(enUS.translation.tools), [])
})
