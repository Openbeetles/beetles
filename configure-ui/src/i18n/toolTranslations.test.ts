import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { zhCN } from './locales/zh-CN.ts'
import { enUS } from './locales/en-US.ts'
import { localizeAccountProviderName } from './providerDisplay.ts'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const registryPath = path.resolve(__dirname, '../../../src/tools/registry.rs')

/** `super::FooTool` 中的 Foo → `GET /api/tools` 的 `name`（与 `handlers/tools.rs` i18n_key 一致） */
const TOOL_CLASS_TO_NAME: Record<string, string> = {
  GetTime: 'get_time',
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
  Diagnose: 'diagnose',
  KvStore: 'kv_store',
  PrivateGarden: 'private_garden',
  FactualMemory: 'factual_memory',
  MemorySearch: 'memory_search',
  MemoryGet: 'memory_get',
  ContinuitySnapshot: 'continuity_snapshot',
  DeviceControl: 'device_control',
  Mail: 'mail',
  ContactsDirectory: 'contacts_directory',
  Documents: 'documents',
  OfficeConfig: 'office_config',
  OfficeStatus: 'office_status',
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
  LuaQuery: 'lua_query',
  LuaDatasheetDistill: 'lua_datasheet_distill',
  LuaProtocolFrameHelper: 'lua_protocol_frame_helper',
  LuaRegisterTableHelper: 'lua_register_table_helper',
  LuaMemoryQuery: 'lua_memory_query',
  LuaStateMachineChecker: 'lua_state_machine_checker',
  LuaToolBridge: 'lua_tool_bridge',
  CapabilityAtomsExchange: 'capability_atoms_exchange',
  CapabilityAtomsInspect: 'capability_atoms_inspect',
}

function registeredToolNames(): string[] {
  const source = fs.readFileSync(registryPath, 'utf8')
  const out = new Set<string>()
  /** 允许 `register(Box::new(` 与 `super::MailTool` 之间换行（office 注册块） */
  const re = /register\(Box::new\(\s*super::([A-Za-z0-9_]+)Tool/g
  for (const match of source.matchAll(re)) {
    const key = TOOL_CLASS_TO_NAME[match[1]]
    if (!key) {
      throw new Error(
        `toolTranslations.test: add TOOL_CLASS_TO_NAME for struct prefix "${match[1]}" (registry.rs)`,
      )
    }
    out.add(key)
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

test('critical office-related tool labels stay aligned with backend semantics', () => {
  assert.equal(zhCN.translation.tools.files, '浏览与读取文件')
  assert.equal(enUS.translation.tools.files, 'Browse & read files')

  assert.equal(zhCN.translation.tools.contacts_directory, '统一联系人与目录（邮件/日历）')
  assert.equal(enUS.translation.tools.contacts_directory, 'Unified people directory (mail/calendar)')

  assert.equal(zhCN.translation.tools.office_status, '办公账户与运行状态')
  assert.equal(enUS.translation.tools.office_status, 'Office accounts & runtime status')
})

test('account provider labels are localized by provider kind with safe fallback', () => {
  const resolveLocaleKey = (
    locale: Record<string, unknown>,
    key: string,
    defaultValue?: string,
  ): string => {
    const resolved = key.split('.').reduce<unknown>((acc, part) => {
      if (!acc || typeof acc !== 'object') return undefined
      return (acc as Record<string, unknown>)[part]
    }, locale)
    return typeof resolved === 'string' ? resolved : (defaultValue ?? key)
  }
  const zhT = (key: string, options?: { defaultValue?: string }) =>
    resolveLocaleKey(zhCN.translation as unknown as Record<string, unknown>, key, options?.defaultValue)
  const enT = (key: string, options?: { defaultValue?: string }) =>
    resolveLocaleKey(enUS.translation as unknown as Record<string, unknown>, key, options?.defaultValue)

  assert.equal(
    localizeAccountProviderName(zhT, 'wecom_documents'),
    '企业微信文档',
  )
  assert.equal(
    localizeAccountProviderName(enT, 'wecom_documents'),
    'WeCom Documents',
  )
  assert.equal(
    localizeAccountProviderName(zhT, 'unknown_provider'),
    'unknown_provider',
  )
})
