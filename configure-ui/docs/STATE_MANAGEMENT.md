# configure-ui 状态分层约定

本约定用于约束 UI 侧状态管理，避免同类状态在多处重复维护。

## 1. 状态分层

- 持久连接状态：`DeviceContext`（`baseUrl` / `pairingCode`），允许 `localStorage` 持久化；同目标重新探测时保留已缓存配对码，只有换目标或设备明确未初始化时才清空。受保护 API 的已验证状态必须绑定到 `baseUrl + pairingCode` 会话；只有同会话或显式验证通过后的“本地无配对码 → 有配对码”提升可以保留 `auth=valid`。
- 全局设备运行状态：`deviceStatusStore`（连接可达性、激活态、重启阶段）。
- 配置主数据：`ConfigContext`（`config` + `load/save`）。
- 页面可编辑副本：仅配置编辑页可保留本地 `form/useState`，用于“编辑未提交”场景。
- 远端异步数据：统一使用 `AsyncState<T>` 形态（`data/loading/error`）。
- 纯 UI 临时态：弹窗开关、输入焦点、折叠展开等，仅允许本地组件状态。
- 未保存状态：`UnsavedContext` 以 owner 集合为权威源；页面/编辑器必须用稳定 owner 写入和清理，禁止无归属地覆盖全局 dirty。
- 异步请求时序：会写入当前编辑对象的请求必须使用 latest-request guard 或等价 request id；旧响应不得覆盖新会话、新技能或新账号的表单状态。

## 2. 强制规则

1. 同一业务数据只允许一个“权威源”（source of truth）。
2. 页面不得直接调用底层 `request()`；统一通过 `useDeviceApi().api.*`。
3. 配置页加载逻辑统一使用 `useConfigPageLoad`，禁止重复写 `loadAttemptedRef` 模板。
4. 保存反馈统一使用 `useSaveFeedback`，保存状态统一为 `idle/saving/ok/fail`。
5. 错误展示优先 i18n key；原始错误文案仅作为兜底。
6. 若页面需要“延迟显示 loading 以避免闪烁”，必须使用可取消的 deferred-loading helper；禁止直接写 `setTimeout(() => setLoading(true), 0)` 后再并行发请求，否则缓存命中/会话合并命中时会出现“数据已渲染但 loading 条卡住”的竞态。
7. 面向设备能力可裁剪的页面（如 `skills` / `tools` / 深观测页）在收到 404 时，必须结合 `GET /` 返回的 endpoint inventory 再判定一次：若根清单里根本没有该接口，应展示“当前设备运行时不支持”而不是裸报 404。
8. 页面主体状态必须互斥：`loading`、阻塞性 `error`、`empty/unsupported/connect-first`、正式内容只能渲染一种。若已有成功数据、后续刷新失败，错误只能退化为顶部 `InlineAlert`，不得和 loading/empty/form 主体并存。
9. 任意重新加载动作开始前必须先清空上一轮页面级错误；禁止保留旧 error 再切回 loading，否则会出现“加载中 + 错误同时可见”的竞态。
10. 页面级 transport / runtime 错误文案必须先归一化为产品语义（i18n key 或统一映射），禁止把 `Failed to fetch`、`operator window required` 一类底层异常直接暴露给用户。
11. 非 `ready` 状态不得再显示“跳去设备页”的弱 blocker；所有受保护页统一复用完整接入卡，让用户就地完成探测、初始化和解锁。
12. 受保护 API 的鉴权结果必须按 `baseUrl + pairingCode` 会话键回写；旧请求不得污染新会话的 auth 状态。
13. 解锁/初始化配对码成功后，前端必须在持久化配对码前立刻提升当前会话为已鉴权；否则会出现“第一次保存成功但仍停在接入卡，第二次点击才进入主视图”的时序错误。
14. 配对码发生“已有值 → 另一个已有值”变化时，必须清空已验证 auth；不能只因 baseUrl 相同就沿用 ready 状态。
15. 弹窗/详情页若允许快速切换对象（如 skill、account），加载响应提交前必须确认仍是最新请求；保存动作不得使用旧对象的内容或字段形状。
16. `dirty=false` 只能清理当前 owner，除非用户明确确认丢弃全局未保存更改；页面卸载时必须清理自己的 owner。

## 3. 新页面接入清单

- 是否可归入 `ConfigContext` / `DeviceContext` / `deviceStatusStore` 之一？
- 是否复用 `useDeviceApi` 作为唯一请求入口？
- 异步请求是否使用 `AsyncState<T>`？
- 会覆盖当前编辑对象的异步请求是否有 latest-request guard？
- 表单 dirty 是否使用稳定 owner 并在卸载时清理？
- 是否避免了重复的加载/保存模板逻辑？

如需新增全局状态，先补充本文件中的“状态分层”和“强制规则”，再落代码。
