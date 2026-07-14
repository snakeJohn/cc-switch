# Grok Build 渠道设计（对标 Codex）

**日期**: 2026-07-14  
**仓库**: snakejohn/cc-switch（fork farion1231/cc-switch）  
**状态**: 已评审（用户确认方案 B 与第 1–4 节设计）

---

## 1. 背景与目标

CC Switch 当前支持 Claude Code、Claude Desktop、Codex、Gemini CLI、OpenCode、OpenClaw、Hermes 七个应用渠道。用户希望在本 fork 中新增 **Grok Build**（xAI CLI）作为第一类应用渠道，**完整对标 Codex**（含 MCP、Skills、代理、tokens），采用与 Codex 相同的 **切换式** live 写入。

Grok Build 配置形态与 Codex 接近：

| 用途 | Grok Build 路径 |
|------|-----------------|
| 配置根目录 | `~/.grok`（可被 `grok_config_dir` 覆盖） |
| 主配置 | `~/.grok/config.toml` |
| 官方认证 | `~/.grok/auth.json`（OAuth/OIDC） |
| Skills | `~/.grok/skills` |
| MCP | `config.toml` 内 `[mcp_servers.*]` |
| 会话日志 | `~/.grok/sessions/` |

自定义模型写在 `[model.<name>]`，支持 `api_backend`: `chat_completions` | `responses` | `messages`。

---

## 2. 范围与非目标

### 2.1 交付范围（分阶段）

| 阶段 | 内容 |
|------|------|
| **P1** | `AppType::Grok` / AppId `grok`；切换式 live 写入；官方 + 第三方预设；表单 UI；配置目录覆盖；备份 |
| **P2** | 统一 MCP 面板同步到 `[mcp_servers.*]`；Skills 同步到 `~/.grok/skills` |
| **P3a** | 应用级代理接管；代理请求写入 `proxy_request_logs`（`app_type=grok`）；用量 UI 过滤 |
| **P3b** | 官方认证体验增强；`session_usage_grok` 无代理会话统计 |

### 2.2 非目标（明确不做）

- 将 Grok 伪装成 Codex 的 `model_providers` 抽象
- 复制 Codex 完整 Responses↔Chat 转换栈（Grok 原生三 backend；默认透传）
- 管理 Grok plugin marketplace / bundled skills
- 写入项目级 `.grok/`（仅用户级 `~/.grok`）
- 在 CCS 内完整自研 xAI OAuth 客户端（P3 最多引导 `grok login` + 读取 auth 状态）
- P1 交付 tokens / MCP / Skills / 代理

---

## 3. 架构总览

```text
┌─────────────────────────────────────────────────────────────┐
│  Frontend: AppId "grok" · GrokFormFields · presets · i18n   │
└────────────────────────────┬────────────────────────────────┘
                             │ Tauri commands (现有 provider/mcp/skill/proxy)
┌────────────────────────────▼────────────────────────────────┐
│  AppType::Grok  ·  MultiAppConfig.providers["grok"]         │
│  settings: current_provider_grok, grok_config_dir           │
└────────────────────────────┬────────────────────────────────┘
                             │
        ┌────────────────────┼────────────────────┐
        ▼                    ▼                    ▼
┌───────────────┐   ┌────────────────┐   ┌──────────────────┐
│ grok_config   │   │ mcp → grok     │   │ skill → ~/.grok/ │
│ 合并写 TOML   │   │ mcp_servers.*  │   │ skills/          │
│ auth 策略     │   └────────────────┘   └──────────────────┘
└───────┬───────┘
        ▼
 ~/.grok/config.toml  ·  auth.json  ·  sessions/ (P3b 只读解析)
```

**写入模式**: `is_additive_mode() = false`（切换式，对齐 Codex）。

**Live 合并原则**: 只修改 CCS 管理的键；永不覆盖用户的 `[ui]`、`[cli]`、`[features]`、`[skills]`、非托管的 `[model.*]`、已有 MCP（供应商切换时）等。

---

## 4. 数据模型

### 4.1 App 标识

- **后端**: `AppType::Grok`，`as_str() => "grok"`
- **前端**: `AppId = ... | "grok"`
- **UI 标签**: 「Grok Build」
- **串解析别名**（可选）: `"grok-build"` → `Grok`（便于兼容）

### 4.2 Provider `settings_config`

```json
{
  "auth": {},
  "config": "<TOML 字符串>",
  "meta": {
    "isOfficial": false,
    "apiBackend": "chat_completions"
  }
}
```

| 字段 | 含义 |
|------|------|
| `auth` | 官方 OAuth 快照（P1 可为空对象）；第三方通常 `{}` |
| `config` | 与该供应商相关的 TOML 片段 / 规范化后的全文策略由实现决定，但 live 投影必须可逆 |
| `meta.isOfficial` | 官方预设；切换时不注入抢 session 的 `api_key` |
| `meta.apiBackend` | `chat_completions` \| `responses` \| `messages` |

### 4.3 Live TOML 约定（第三方）

固定 slot **`cc-switch-active`**，避免切换时堆积无限 `[model.*]`：

```toml
[models]
default = "cc-switch-active"

[model.cc-switch-active]
name = "Provider Display Name"
model = "model-id"
base_url = "https://api.example.com/v1"
api_key = "sk-..."
api_backend = "chat_completions"
```

**官方**:

- `models.default` → 内置模型（如 `grok-build`）
- 不写会抢过 `auth.json` session 的 active `api_key`
- 不删除用户 `auth.json`

### 4.4 路径解析

优先级：

1. CCS settings `grok_config_dir`（显式覆盖）
2. 默认 `~/.grok`（跨平台 home）

备份目录：`~/.cc-switch/backups/grok/`（或项目现有 `get_app_config_dir()/backups/grok`）。

---

## 5. P1：核心 App 与供应商切换

### 5.1 后端模块

| 模块 | 职责 |
|------|------|
| `grok_config.rs` | 路径、读 TOML、原子写、备份、合并 upsert/remove table |
| `services/config.rs` | `sync_grok_live`；`sync_current_providers_to_live` 纳入 Grok |
| `app_config.rs` | `AppType::Grok`；MCP/Skills apps 预留 `grok: bool` |
| `settings.rs` | `grok_config_dir`、`current_provider_grok` |
| provider 服务 | 复用现有 CRUD/switch；switch 触发 live 同步 |
| exhaustive match | 全仓库 `AppType` / `AppId` 补全；MCP/Skills/Proxy 未实现分支 `skip` + log |

### 5.2 Live 同步流程

1. 从 DB 取当前 provider 的 `settings_config`
2. 备份现有 `config.toml`（必要时 `auth.json`）
3. 合并写入供应商相关段（见 §4.3）
4. 官方：auth 策略见 §7.2（P1 最小：不破坏现有 auth.json）
5. 更新 `current_provider_grok`

### 5.3 前端

| 项 | 内容 |
|----|------|
| `APP_IDS` / `APP_ICON_MAP` | 注册 grok |
| `grokProviderPresets.ts` | 官方 + 自定义 OpenAI 兼容 + 可选 1–2 中转模板 |
| `GrokFormFields` | name、website、apiKey、baseUrl、model、apiBackend；官方隐藏 key |
| Add/Edit dialog | `app === "grok"` 分支 |
| Settings | Grok 配置目录覆盖 |
| i18n | 至少 zh + en |
| Tray / AppSwitcher | 与 Codex 同级可见（若 tray 有白名单则纳入） |

### 5.4 预设最小集

1. **xAI 官方** — `isOfficial: true`；提示 `grok login` / `XAI_API_KEY`
2. **自定义 OpenAI 兼容** — 用户填 base_url / key / model；默认 `chat_completions`
3. 可选中转模板 — 空 key + 占位 endpoint

辅助：`generateGrokThirdPartyConfig(...)` → TOML 字符串。

### 5.5 P1 验收

1. App 切换器可见 Grok Build  
2. 第三方切换后 live 出现 `default` + `cc-switch-active`，用户 `[ui]` 等保留  
3. 官方切换不注入抢 session 的 api_key  
4. 配置目录覆盖生效  
5. 写前备份  
6. 其它 app 回归无破坏  

---

## 6. P2：MCP 与 Skills

### 6.1 MCP

- **目标文件**: `~/.grok/config.toml` 的 `[mcp_servers.<id>]`
- **服务**: `sync_single_server_to_grok` / `remove_server_from_grok`；`McpService` 全路径加 `AppType::Grok`
- **DB**: `apps.grok: bool`
- **合并**: 只动 MCP 相关 section；与 P1 共用 TOML 工具
- **前端**: `MCP_APP_IDS` 含 `grok`
- **字段映射**: stdio (`command`/`args`/`env`)；HTTP (`url`/`headers`)；`enabled`；Grok 不支持的扩展键忽略

**验收**: 勾选/取消/删除仅影响对应 section；与其它 app 并存无互相覆盖。

### 6.2 Skills

- **投影目录**: `~/.grok/skills/<name>/`（尊重 `grok_config_dir`）
- **范围**: 仅 CCS 安装的 skill；不管理 bundled/plugin/项目级 `.grok/skills`
- **服务**: `get_app_skills_dir(Grok)`；安装/启用/禁用同步沿用现有 SSOT 策略
- **前端**: `SKILLS_APP_IDS` 含 `grok`

**验收**: 启用出现目录；禁用按现有策略移除；不删 Grok 自带技能。

### 6.3 P2 不做

代理、OAuth UI、tokens session 解析、plugin marketplace。

---

## 7. P3：代理、官方认证、Tokens

### 7.1 代理接管（P3a）

1. Live 当前 model 的 `base_url` → 本地代理（如 `http://127.0.0.1:<port>/v1`）
2. Live `api_key` → 代理占位 / 托管值；真实上游凭证在 DB
3. 代理按 `api_backend` **透传** 三种协议；默认不做 Chat↔Responses 互转
4. 关闭接管后恢复真实 base_url/key
5. 日志 `app_type = "grok"` 写入 `proxy_request_logs`

**验收**: CLI 经代理可达上游；日志可筛 grok；关闭后 live 恢复。

### 7.2 官方认证增强（P3b）

| 能力 | 说明 |
|------|------|
| 官方切换 | 不写抢 session 的 api_key；default 回内置模型；不删 auth.json |
| 第三方 → 官方 | default 不再指向 `cc-switch-active`（或停用该 slot 作为 default） |
| 状态展示（可选） | 解析 auth.json 显示已登录/未登录/将过期 |
| 引导登录（可选） | 打开文档或调用 `grok login`；不自研完整 OAuth |

### 7.3 Tokens 统计

**通用（无需重做）**:

- 存储表 `proxy_request_logs`、汇总/趋势 API、费用计算引擎
- 有 `app_type=grok` 的行即可在 Dashboard 展示

**Grok 单独需要**:

| 项 | 阶段 | 说明 |
|----|------|------|
| 代理日志打标 | P3a | 有代理即有用量 |
| UI 过滤 / 主题 / i18n | P3a | `usage.appFilter.grok` 等 |
| 默认模型定价 | P3a | `grok-build` 等 |
| `session_usage_grok` | P3b | 解析 `~/.grok/sessions/**`；`data_source=grok_session`；去重避免与 proxy 双计 |

**说明**: 会话格式与 Codex/Claude 不同，必须独立解析器；P1/P2 不交付 tokens。

---

## 8. 错误处理与安全

- 写文件失败：返回可本地化 `AppError`；不留下半截文件（原子写）
- TOML 解析失败：拒绝写入并提示用户修复/从备份恢复
- API Key：DB 与 live 按现有应用同样密文/权限策略；日志不打印完整 key
- 备份保留数量：对齐 `effective_backup_retain_count`
- 代理占位 key 不得被误当作上游凭证外发（对齐 Codex `PROXY_MANAGED` 语义）

---

## 9. 测试策略

| 层级 | 内容 |
|------|------|
| 单元 | `grok_config` 合并：保留 ui、更新 active model、删除 mcp section |
| 单元 | `AppType` 解析 / additive=false |
| 集成 | switch provider → 磁盘 TOML 快照 |
| 集成 | MCP upsert/remove 与 model 段共存 |
| P3 | 代理打标；session 解析 fixture；去重 |
| 回归 | Claude/Codex/Gemini 现有测试仍通过 |

---

## 10. 实现触点清单（防漏）

实现时需穷举并更新（非完整列表，以编译与运行时路径为准）：

**后端**: `app_config.rs`, `settings.rs`, `services/config.rs`, `services/mcp.rs`, `services/skill.rs`, `services/profile.rs`, `tray.rs`, `proxy/*`, `commands/*`, `database/*`（MCP apps / providers）, `lib.rs`

**前端**: `lib/api/types.ts`, `config/appConfig.tsx`, `config/grokProviderPresets.ts`, `components/providers/**`, `components/usage/**`, `i18n/**`, settings 目录表单

**P1 对未实现分支**: match 加臂并 skip，保证编译通过。

---

## 11. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 全量改 AppType 触点多 | 分 P1–P3；P1 先编译绿 + live 切换正确 |
| 合并写破坏用户 config | 单测 + 备份 + 只改托管键 |
| Grok session 格式变更 | P3b 解析容错；缺字段 skip |
| 代理协议差异 | 透传三 backend；个案再加转换 |
| 与上游 farion 合并冲突 | fork 内独立模块 `grok_*`，减少侵入式重构 |

---

## 12. 成功标准（整体）

用户可在 CC Switch 中像使用 Codex 一样管理 Grok Build：

1. 多供应商切换并正确投影到 `~/.grok`
2. MCP / Skills 统一面板可勾选 Grok
3. 可选代理接管与用量统计（代理 + 可选会话同步）
4. 不破坏用户既有 Grok UI/MCP/其它配置

---

## 13. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-07-14 | 初稿：方案 B；切换式；P1–P3；tokens 通用存储 + Grok 需单独接入 |
