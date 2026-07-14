# Grok Build Channel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 CC Switch fork 中新增第一类应用渠道 `grok`（UI：Grok Build），切换式写入 `~/.grok/config.toml`，并分阶段对齐 Codex 的 MCP、Skills、代理与 tokens。

**Architecture:** 后端新增 `AppType::Grok` + `grok_config.rs`（TOML 合并写、备份）；供应商 `settings_config` 为 `{ auth, config, meta }`；live 使用固定 model slot `cc-switch-active`。前端注册 AppId、预设与表单。P2 复用 MCP/Skills 统一面板；P3 代理透传三 backend + 可选 session 用量解析。

**Tech Stack:** Tauri 2 + Rust (`toml_edit`, `serde_json`)、React/TS、现有 Provider/MCP/Skill/Proxy 服务。

**Spec:** `docs/superpowers/specs/2026-07-14-grok-build-channel-design.md`

## Global Constraints

- App id 字符串固定为 `grok`；UI 标签为 `Grok Build`。
- `is_additive_mode() = false`（切换式，对齐 Codex）。
- Live 合并只改 CCS 托管键；永不清空用户 `[ui]` / 非托管 `[model.*]` / 无关 MCP。
- 第三方 live slot 固定为 `[model.cc-switch-active]`，`[models].default = "cc-switch-active"`。
- 官方供应商：不向 live 写入会抢 session 的 `api_key`；不删除 `auth.json`。
- Tokens：存储层通用；P1/P2 不交付 session 解析；P3a 代理日志 + UI 过滤；P3b `session_usage_grok`。
- 代理默认透传 `chat_completions` / `responses` / `messages`，不做 Codex 式协议转换。
- 工作区若无 `.git`，跳过 commit 步骤；有 git 时按 step 提交。
- 每任务结束：相关 `cargo test` / 前端类型检查通过后再进入下一任务。

---

## File Map

### Create

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/grok_config.rs` | 路径、备份、TOML 合并、第三方/官方 live 投影 |
| `src-tauri/src/mcp/grok.rs` | MCP upsert/remove for Grok (P2) |
| `src-tauri/src/services/session_usage_grok.rs` | 会话 tokens 同步 (P3b) |
| `src/config/grokProviderPresets.ts` | 官方 + 自定义预设 |
| `src/components/providers/forms/GrokFormFields.tsx` | 表单字段 |
| `src/components/providers/forms/hooks/useGrokFormState.ts` | 表单状态与 settings_config 序列化 |
| `src-tauri/tests/grok_config_merge.rs` | 合并写集成测试（tempdir） |

### Modify (P1 critical)

| Path | Change |
|------|--------|
| `src-tauri/src/app_config.rs` | `AppType::Grok`；`McpApps`/`SkillApps` 加 `grok`；managers 默认项 |
| `src-tauri/src/lib.rs` | `mod grok_config` |
| `src-tauri/src/settings.rs` | `grok_config_dir`、`current_provider_grok`、getter/setter |
| `src-tauri/src/services/config.rs` | `sync_grok_live` |
| `src-tauri/src/services/provider/live.rs` | `write_live` 分支 Grok |
| `src-tauri/src/provider.rs` | usage credentials / 校验若按 app 分支则加 Grok |
| 所有 `AppType` exhaustive match | 补 `Grok` 臂（未实现则 log skip） |
| `src/lib/api/types.ts` | `AppId` 加 `"grok"` |
| `src/config/appConfig.tsx` | APP_IDS / 图标 / 主题 |
| `src/components/providers/forms/ProviderForm.tsx` | grok 分支 |
| `src/i18n/locales/zh.json` / `en.json` | 文案 |
| Settings 目录 UI | Grok 配置目录项 |

### Modify (P2+)

| Path | Change |
|------|--------|
| `src-tauri/src/mcp/mod.rs` + `services/mcp.rs` | 导出/同步 Grok |
| `src-tauri/src/services/skill.rs` | skills 目录 |
| `src/config/appConfig.tsx` | `MCP_APP_IDS` / `SKILLS_APP_IDS` |
| Proxy / usage UI | P3 |

---

### Task 1: AppType::Grok + 解析测试

**Files:**
- Modify: `src-tauri/src/app_config.rs`
- Modify: `src-tauri/tests/app_type_parse.rs`
- Modify: `src-tauri/src/lib.rs`（仅若需导出，本任务可不改）

**Interfaces:**
- Produces: `AppType::Grok`, `as_str() == "grok"`, `FromStr` 接受 `"grok"` / `"grok-build"`, `is_additive_mode() == false`

- [ ] **Step 1: 扩展失败测试**

在 `src-tauri/tests/app_type_parse.rs` 追加：

```rust
#[test]
fn parse_grok_app_ids() {
    assert!(matches!(AppType::from_str("grok"), Ok(AppType::Grok)));
    assert!(matches!(AppType::from_str("GROK"), Ok(AppType::Grok)));
    assert!(matches!(AppType::from_str("grok-build"), Ok(AppType::Grok)));
    assert!(!AppType::Grok.is_additive_mode());
    assert_eq!(AppType::Grok.as_str(), "grok");
}
```

- [ ] **Step 2: 运行测试，确认失败**

```powershell
cd src-tauri
cargo test --test app_type_parse parse_grok_app_ids -- --nocapture
```

Expected: compile error or FAIL（`Grok` 不存在）

- [ ] **Step 3: 实现 `AppType::Grok`**

在 `app_config.rs` 的 `enum AppType` 增加 `Grok`。更新：

```rust
// as_str
AppType::Grok => "grok",

// is_additive_mode — Grok 不在 additive 列表中

// all()
AppType::Grok,

// FromStr
"grok" | "grok-build" | "grok_build" | "grokbuild" => Ok(AppType::Grok),
// error message Allowed 列表加入 grok
```

同步更新本文件内所有 `match app` / `match app_type`（`McpApps`、`SkillApps`、`CommonConfigSnippets`、`get_manager` 等）：

```rust
// McpApps / SkillApps 结构体字段
#[serde(default)]
pub grok: bool,

// is_enabled_for / set_enabled_for / enabled_apps / is_empty 增加 grok
AppType::Grok => self.grok,
// ...
AppType::Grok => self.grok = enabled,
// enabled_apps: if self.grok { apps.push(AppType::Grok); }
// is_empty: && !self.grok
```

`MultiAppConfig` 默认 managers：

```rust
apps.insert("grok".to_string(), ProviderManager::default());
```

对暂无逻辑的 match 臂使用：

```rust
AppType::Grok => { /* P1: no-op or skip */ }
```

保证 **`cargo check` 通过**。可用：

```powershell
cd src-tauri
cargo check 2>&1
```

修复所有 non-exhaustive pattern 错误（全 crate）。

- [ ] **Step 4: 跑测试**

```powershell
cd src-tauri
cargo test --test app_type_parse -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit（有 git 时）**

```bash
git add src-tauri/src/app_config.rs src-tauri/tests/app_type_parse.rs
git commit -m "feat(grok): add AppType::Grok and parse aliases"
```

---

### Task 2: `grok_config.rs` 路径 + 合并写 + 单测

**Files:**
- Create: `src-tauri/src/grok_config.rs`
- Modify: `src-tauri/src/lib.rs` — `mod grok_config;`
- Modify: `src-tauri/src/settings.rs` — `grok_config_dir` + `get_grok_override_dir`
- Create: `src-tauri/tests/grok_config_merge.rs`（或模块内 `#[cfg(test)]`）

**Interfaces:**
- Produces:
  - `pub fn get_grok_dir() -> PathBuf`
  - `pub fn get_grok_config_path() -> PathBuf`
  - `pub fn get_grok_auth_path() -> PathBuf`
  - `pub const GROK_ACTIVE_MODEL_ID: &str = "cc-switch-active"`
  - `pub fn write_grok_provider_live(settings_config: &Value, is_official: bool) -> Result<(), AppError>`
  - `pub fn generate_third_party_model_toml(name, model, base_url, api_key, api_backend) -> String`（可选，测试友好）

- [ ] **Step 1: settings 增加覆盖目录**

在 `AppSettings`（`settings.rs`）增加：

```rust
pub grok_config_dir: Option<String>,
pub current_provider_grok: Option<String>,
```

默认 `None`；在 load/normalize 中 trim 空字符串→None。

```rust
pub fn get_grok_override_dir() -> Option<PathBuf> {
    // 对齐 get_hermes_override_dir / get_codex_override_dir
}
```

`get_current_provider` / `set_current_provider` match 增加 Grok。

- [ ] **Step 2: 写失败测试（合并保留 ui）**

在 `grok_config.rs` 底部 `#[cfg(test)]`：

```rust
#[test]
fn merge_preserves_ui_and_writes_active_model() {
    let dir = tempfile::tempdir().unwrap();
    // 通过 env 或测试专用 inject 设置 grok dir 到 dir.path()
    // 预置 config.toml:
    // [ui]\nyolo = true\n[models]\ndefault = "old"\n
    // 调用 write_grok_provider_live 第三方配置
    // assert 文件仍含 yolo = true
    // assert default == "cc-switch-active"
    // assert [model.cc-switch-active] 含 base_url 与 api_key
}
```

测试环境注入策略（二选一，与 Hermes 测试一致优先）：

- 临时写 `AppSettings.grok_config_dir` 再调 `get_grok_dir`；或
- `write_grok_provider_live_at(path, ...)` 纯函数路径供测。

**推荐**：内部 `fn apply_provider_to_doc(doc: &mut DocumentMut, ...)` 纯函数 + 单测不碰全局 settings。

- [ ] **Step 3: 实现核心逻辑**

```rust
// grok_config.rs 骨架
use toml_edit::{DocumentMut, value, Item, Table};
use crate::error::AppError;
use crate::config::{atomic_write, get_app_config_dir};
use crate::settings::{effective_backup_retain_count, get_grok_override_dir};

pub const GROK_ACTIVE_MODEL_ID: &str = "cc-switch-active";

pub fn get_grok_dir() -> PathBuf {
    if let Some(d) = get_grok_override_dir() {
        return d;
    }
    crate::config::get_home_dir().join(".grok")
}

pub fn get_grok_config_path() -> PathBuf {
    get_grok_dir().join("config.toml")
}

pub fn get_grok_auth_path() -> PathBuf {
    get_grok_dir().join("auth.json")
}

/// 从 settings_config 投影到 live config.toml
pub fn write_grok_provider_live(
    settings_config: &serde_json::Value,
    is_official: bool,
) -> Result<(), AppError> {
    let path = get_grok_config_path();
    backup_grok_config_if_exists(&path)?;

    let mut doc = read_or_empty_doc(&path)?;

    if is_official {
        apply_official(&mut doc, settings_config)?;
    } else {
        apply_third_party(&mut doc, settings_config)?;
    }

    atomic_write(&path, doc.to_string())?;
    Ok(())
}

fn apply_third_party(doc: &mut DocumentMut, settings: &serde_json::Value) -> Result<(), AppError> {
    // 优先从 meta / 结构化字段读取；否则解析 settings["config"] TOML 字符串
    // 设置:
    // doc["models"]["default"] = "cc-switch-active"
    // doc["model"][GROK_ACTIVE_MODEL_ID] = table with model, base_url, api_key, api_backend, name
    Ok(())
}

fn apply_official(doc: &mut DocumentMut, settings: &serde_json::Value) -> Result<(), AppError> {
    // models.default = settings 指定或 "grok-build"
    // 若存在 model.cc-switch-active，删除或保留但不设为 default
    // 不写 api_key 到内置模型覆盖（避免抢 session）
    Ok(())
}
```

`settings_config` 约定（写入端在 Task 4/5 对齐）：

```json
{
  "auth": {},
  "config": "<optional full/snippet toml>",
  "meta": {
    "isOfficial": false,
    "apiBackend": "chat_completions",
    "model": "deepseek-chat",
    "baseUrl": "https://...",
    "apiKey": "sk-...",
    "displayName": "DeepSeek"
  }
}
```

**实现要求：** `meta` 结构化字段优先；若缺则从 `config` TOML 解析 `model.cc-switch-active` / 顶层。

备份：

```rust
fn backup_grok_config_if_exists(path: &Path) -> Result<(), AppError> {
    if !path.exists() { return Ok(()); }
    let backup_dir = get_app_config_dir().join("backups").join("grok");
    std::fs::create_dir_all(&backup_dir)?;
    // 时间戳文件名 + 按 retain 清理，对齐 hermes/codex
    Ok(())
}
```

- [ ] **Step 4: 注册模块**

`lib.rs`:

```rust
mod grok_config;
```

- [ ] **Step 5: 跑单测**

```powershell
cd src-tauri
cargo test grok_config -- --nocapture
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/grok_config.rs src-tauri/src/lib.rs src-tauri/src/settings.rs
git commit -m "feat(grok): add grok_config live merge and settings override"
```

---

### Task 3: Provider live 同步接线

**Files:**
- Modify: `src-tauri/src/services/config.rs`
- Modify: `src-tauri/src/services/provider/live.rs`
- Modify: `src-tauri/src/provider.rs`（若有 per-app validate/credentials）

**Interfaces:**
- Consumes: `grok_config::write_grok_provider_live`
- Produces: switch/update 当前 Grok 供应商时磁盘 config 更新

- [ ] **Step 1: `sync_grok_live`**

在 `services/config.rs`：

```rust
fn sync_grok_live(
    _config: &mut MultiAppConfig,
    provider_id: &str,
    provider: &Provider,
) -> Result<(), AppError> {
    let is_official = provider
        .settings_config
        .pointer("/meta/isOfficial")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || provider.category.as_deref() == Some("official");

    crate::grok_config::write_grok_provider_live(&provider.settings_config, is_official)?;
    Ok(())
}
```

在 `sync_current_provider_for_app` 的 match 增加：

```rust
AppType::Grok => Self::sync_grok_live(config, &current_id, &provider)?,
```

`sync_current_providers_to_live` 增加对 Grok 的调用（与 Claude/Codex/Gemini 并列）。

- [ ] **Step 2: `write_live_with_common_config` 分支**

在 `services/provider/live.rs` 的 app match 增加：

```rust
AppType::Grok => {
    let is_official = /* same as above */;
    crate::grok_config::write_grok_provider_live(&provider.settings_config, is_official)?;
}
```

- [ ] **Step 3: validate（如有）**

若 `ProviderService::validate_provider_settings` 按 app 校验，为 Grok 要求：

- `settings_config` 为 object
- 非官方：`meta.baseUrl` 或 config 中可解析出 base_url
- 官方：放宽 api_key

- [ ] **Step 4: 集成冒烟（手动或测试）**

```powershell
cd src-tauri
cargo test --lib provider -- --nocapture
cargo check
```

可选：在 `tests/` 用 tempdir + override 写一个 switch 后读文件断言。

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(grok): wire provider live sync to grok_config"
```

---

### Task 4: 前端 AppId + appConfig + i18n

**Files:**
- Modify: `src/lib/api/types.ts`
- Modify: `src/config/appConfig.tsx`
- Modify: `src/i18n/locales/zh.json`, `en.json`（`ja.json` 若存在则同步键）
- Modify: Settings 中配置目录相关组件（搜 `hermesConfigDir`）

**Interfaces:**
- Produces: UI 可选中 Grok Build；类型系统识别 `"grok"`

- [ ] **Step 1: 类型**

```ts
// src/lib/api/types.ts
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "grok";
```

- [ ] **Step 2: appConfig**

```tsx
// APP_IDS 数组加入 "grok"
// APP_ICON_MAP.grok:
{
  label: "Grok Build",
  icon: <ProviderIcon icon="xai" name="Grok" size={14} showFallback={false} /> // 若无 xai 图标用字母 fallback
  activeClass: "bg-neutral-500/10 ring-1 ring-neutral-500/20 ...",
  badgeClass: "bg-neutral-500/10 text-neutral-700 ...",
}
// P1: SKILLS_APP_IDS / MCP_APP_IDS 暂不加入（P2 再加）
```

若 `ProviderIcon` 无 xai，使用现有通用 fallback 或 `BrandIcons` 新增简单 SVG。

- [ ] **Step 3: i18n 最小键**

`zh.json` / `en.json`（路径对齐现有 `apps` / `settings`）：

```json
"apps": {
  "grok": "Grok Build"
},
"settings": {
  "grokConfigDir": "Grok Build 配置目录",
  "grokConfigDirDescription": "覆盖 Grok Build 配置目录（config.toml / auth.json）。",
  "browsePlaceholderGrok": "例如：C:\\Users\\<你>\\.grok"
},
"provider": {
  "grokOfficialNoApiKey": "官方无需填写 API Key，请在本机执行 grok login",
  "grokApiHint": "填写 OpenAI 兼容端点；api_backend 可选 chat_completions / responses / messages"
}
```

- [ ] **Step 4: Settings 目录覆盖 UI**

找到 Hermes/Codex 配置目录表单项，复制一条绑定 `grokConfigDir` ↔ settings `grok_config_dir`（字段名与 Rust `#[serde(rename_all = "camelCase")]` 对齐，一般为 `grokConfigDir`）。

- [ ] **Step 5: 前端类型检查**

```powershell
pnpm exec tsc --noEmit
```

修复因 `AppId` 穷尽 match 导致的错误（switch/app 列表等）。

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(grok): register frontend AppId and settings dir override"
```

---

### Task 5: 预设 + 表单 + ProviderForm 接线

**Files:**
- Create: `src/config/grokProviderPresets.ts`
- Create: `src/components/providers/forms/hooks/useGrokFormState.ts`
- Create: `src/components/providers/forms/GrokFormFields.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `ProviderPresetSelector` / Add dialog 若按 app 取 presets

**Interfaces:**
- Produces: 可添加/编辑 Grok 供应商，`settings_config` 符合 Task 2 约定

- [ ] **Step 1: 预设文件**

```ts
// src/config/grokProviderPresets.ts
export type GrokApiBackend =
  | "chat_completions"
  | "responses"
  | "messages";

export interface GrokProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    auth: Record<string, unknown>;
    config: string;
    meta: {
      isOfficial?: boolean;
      apiBackend?: GrokApiBackend;
      model?: string;
      baseUrl?: string;
      apiKey?: string;
      displayName?: string;
    };
  };
  isOfficial?: boolean;
  category?: "official" | "cn_official" | "aggregator" | "custom";
  icon?: string;
  endpointCandidates?: string[];
}

export function generateGrokThirdPartyConfig(opts: {
  displayName: string;
  model: string;
  baseUrl: string;
  apiKey: string;
  apiBackend: GrokApiBackend;
}): { auth: Record<string, unknown>; config: string; meta: GrokProviderPreset["settingsConfig"]["meta"] } {
  const apiBackend = opts.apiBackend || "chat_completions";
  const config = `[models]
default = "cc-switch-active"

[model.cc-switch-active]
name = ${JSON.stringify(opts.displayName)}
model = ${JSON.stringify(opts.model)}
base_url = ${JSON.stringify(opts.baseUrl)}
api_key = ${JSON.stringify(opts.apiKey)}
api_backend = ${JSON.stringify(apiBackend)}
`;
  return {
    auth: {},
    config,
    meta: {
      isOfficial: false,
      apiBackend,
      model: opts.model,
      baseUrl: opts.baseUrl,
      apiKey: opts.apiKey,
      displayName: opts.displayName,
    },
  };
}

export const grokProviderPresets: GrokProviderPreset[] = [
  {
    name: "xAI Official",
    nameKey: "presets.grokOfficial",
    websiteUrl: "https://grok.com",
    isOfficial: true,
    category: "official",
    settingsConfig: {
      auth: {},
      config: `[models]\ndefault = "grok-build"\n`,
      meta: {
        isOfficial: true,
        model: "grok-build",
        apiBackend: "responses",
      },
    },
  },
  {
    name: "Custom",
    websiteUrl: "",
    category: "custom",
    settingsConfig: generateGrokThirdPartyConfig({
      displayName: "Custom",
      model: "grok-build",
      baseUrl: "https://api.x.ai/v1",
      apiKey: "",
      apiBackend: "chat_completions",
    }),
  },
];
```

- [ ] **Step 2: `useGrokFormState`**

管理 `baseUrl`、`apiKey`、`model`、`apiBackend`、`isOfficial`；变更时 `onSettingsConfigChange(JSON.stringify({ auth, config, meta }))`。

- [ ] **Step 3: `GrokFormFields`**

- 官方：提示 `provider.grokOfficialNoApiKey`，隐藏 API Key  
- 第三方：ApiKey + BaseUrl + Model + Select(apiBackend)  
- 复用 `ApiKeyInput` / `EndpointField` 若已有  

- [ ] **Step 4: `ProviderForm` 分支**

对齐 hermes/codex：

```tsx
import { grokProviderPresets } from "@/config/grokProviderPresets";
import { GrokFormFields } from "./GrokFormFields";
import { useGrokFormState } from "./hooks/useGrokFormState";

// presets: appId === "grok" ? grokProviderPresets : ...
// render: appId === "grok" && <GrokFormFields ... />
// submit: 确保 settingsConfig 为对象（不要双重 stringify）
```

检查 `providerSchema` 是否允许 grok 的 settings 形状；必要时放宽。

- [ ] **Step 5: 手动验收清单**

1. 启动 app（`pnpm tauri dev` 或项目文档命令）  
2. 切换到 Grok Build  
3. 添加 Custom，填 key/url，保存并切换  
4. 打开 `~/.grok/config.toml` 确认 `cc-switch-active`  
5. 切官方，确认 default 非强制带第三方 api_key  

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(grok): presets and provider form for Grok Build"
```

---

### Task 6: P1 收尾 — tray、deeplink、visible apps、回归

**Files:**
- Modify: `src-tauri/src/tray.rs`（若菜单列 app）
- Modify: `src-tauri/src/deeplink/parser.rs` — 允许 app=`grok`
- Modify: `src-tauri/src/settings.rs` visible apps 默认含 grok（若有白名单）
- Modify: `src-tauri/src/commands/misc.rs` 工具列表（P1 可选：不装 grok CLI 管理）
- 全量编译修复

- [ ] **Step 1: deeplink 白名单**

```rust
"claude" | "codex" | "gemini" | "opencode" | "openclaw" | "hermes" | "grok"
```

- [ ] **Step 2: tray**

若 tray 硬编码 Claude/Codex/Gemini，P1 至少在应用内 AppSwitcher 可用；tray 可加 Grok 项或延后。若加：

```rust
TrayApp { app_type: AppType::Grok, /* label */ }
```

- [ ] **Step 3: 回归**

```powershell
cd src-tauri
cargo test --tests app_type_parse
cargo test grok_config
cargo check
cd ..
pnpm exec tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(grok): P1 integration polish and deeplink support"
```

**P1 完成标准：** 设计 §5.5 六条验收全部满足。

---

### Task 7: P2 MCP 同步

**Files:**
- Create: `src-tauri/src/mcp/grok.rs`
- Modify: `src-tauri/src/mcp/mod.rs`
- Modify: `src-tauri/src/services/mcp.rs`
- Modify: `src/config/appConfig.tsx` — `MCP_APP_IDS`
- Frontend MCP 多选（自动随 APP 列表时确认）

**Interfaces:**
- Produces: `sync_single_server_to_grok`, `remove_server_from_grok`（可仿 `mcp/codex.rs`）

- [ ] **Step 1: 实现 `mcp/grok.rs`**

几乎照抄 `sync_single_server_to_codex`，路径改为 `grok_config::get_grok_config_path()`，section 仍为 `mcp_servers`（Grok 官方文档即此名）。

复用 `json_server_to_toml_table`：若该函数在 codex 模块私有，则：

- 抽到 `mcp/toml_common.rs`，或  
- 在 grok.rs 复制最小转换（command/args/env/url/headers/enabled）

```rust
pub fn sync_single_server_to_grok(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> { /* merge mcp_servers.id */ }

pub fn remove_server_from_grok(id: &str) -> Result<(), AppError> { /* remove key */ }
```

- [ ] **Step 2: McpService 接线**

所有 `prev_apps.hermes` 旁加 `grok`；match `AppType::Grok => sync/remove_grok`。

- [ ] **Step 3: 前端**

```ts
export const MCP_APP_IDS: AppId[] = [..., "grok"];
```

- [ ] **Step 4: 测试**

单测：空 doc upsert 后含 `[mcp_servers.x]`；remove 后无 x；预置 `[ui]` 仍在。

```powershell
cargo test mcp::grok -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(grok): MCP sync to ~/.grok/config.toml"
```

---

### Task 8: P2 Skills 同步

**Files:**
- Modify: `src-tauri/src/services/skill.rs` — `get_app_skills_dir`
- Modify: `src/config/appConfig.tsx` — `SKILLS_APP_IDS`
- SkillApps 已在 Task 1 含 `grok`

- [ ] **Step 1: 目录**

```rust
AppType::Grok => {
    if let Some(custom) = crate::settings::get_grok_override_dir() {
        return Ok(custom.join("skills"));
    }
}
// default:
AppType::Grok => crate::grok_config::get_grok_dir().join("skills"),
```

- [ ] **Step 2: 前端 SKILLS_APP_IDS 加入 `"grok"`**

- [ ] **Step 3: 手动：安装 skill 勾选 Grok → 检查 `~/.grok/skills`**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(grok): skills directory sync for Grok Build"
```

---

### Task 9: P3a 代理接管 + 用量 UI

**Files:**
- Modify: `src-tauri/src/services/proxy.rs` 及 `proxy/providers/*`（按现有 per-app 模式）
- Modify: `grok_config.rs` — takeover 时改写 active model base_url/api_key
- Modify: `src/components/usage/UsageHero.tsx`, `UsageDashboard.tsx`, i18n
- Pricing 默认模型（可选）

**Interfaces:**
- 代理请求日志 `app_type = "grok"`
- Live：`base_url = http://127.0.0.1:<port>/v1`，key 占位

- [ ] **Step 1: 调研现有 Gemini/Claude takeover 写入点，复制最小 Grok 路径**

在 `ProxyService` 中 `AppType::Grok`：

- 启动接管：读当前 provider meta → 写 live 指向本地代理  
- 转发：按 `api_backend` 选路径透传  
- 停止：`write_grok_provider_live` 恢复真实配置  

- [ ] **Step 2: 用量 UI**

```ts
// UsageHero TITLE_THEMES
grok: { accent: "text-neutral-700 dark:text-neutral-300", iconBg: "bg-neutral-500/10" }

// UsageDashboard app 列表含 grok
// i18n usage.appFilter.grok = "Grok Build"
```

- [ ] **Step 3: 验收**

开启代理后请求一次；用量页筛选 Grok 有数据。

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(grok): proxy takeover and usage app filter"
```

---

### Task 10: P3b 官方认证体验 + session_usage_grok

**Files:**
- Create: `src-tauri/src/services/session_usage_grok.rs`
- Modify: `src-tauri/src/commands/usage.rs` — `sync_session_usage` 调用 grok
- Modify: 官方预设 UI 状态（可选读 auth.json）

- [ ] **Step 1: 用真实 `~/.grok/sessions/**/events.jsonl` 做 fixture 分析 token 字段**

记录字段路径到模块注释；只解析高置信 usage 事件。

- [ ] **Step 2: 实现同步**

```rust
pub fn sync_grok_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    // 扫描 get_grok_dir().join("sessions")
    // 增量 + DedupKey + data_source "grok_session" + app_type "grok"
}
```

在 `sync_session_usage` 中 `match` 追加调用（best-effort，失败 push errors）。

- [ ] **Step 3: 官方 UI**

显示 auth.json 是否存在；按钮文案「请运行 grok login」；不实现完整 OAuth。

- [ ] **Step 4: 测试 fixture + cargo test**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(grok): session usage sync and official auth status"
```

---

## Spec Coverage Check

| Spec 章节 | Task |
|-----------|------|
| §4 数据模型 / AppId | 1, 2, 4 |
| §5 P1 live / 表单 / 预设 / 目录 | 2, 3, 4, 5, 6 |
| §6 P2 MCP / Skills | 7, 8 |
| §7 P3 代理 / 认证 / tokens | 9, 10 |
| §8 错误处理 / 备份 | 2（atomic + backup） |
| §9 测试 | 各 task 内 |
| Tokens 通用存储说明 | 9–10；P1 不交付 |

## Placeholder / Consistency Notes

- Live slot 名全程 `cc-switch-active` / `GROK_ACTIVE_MODEL_ID`。
- `meta.isOfficial` 与 `category == "official"` 双识别，避免旧数据漏判。
- MCP section 名与 Grok 文档一致：`mcp_servers`。
- 无 git 时跳过所有 commit step，不阻断实现。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-14-grok-build-channel.md`.

**Two execution options:**

1. **Subagent-Driven（推荐）** — 每任务新开 subagent，任务间 review  
2. **Inline Execution** — 本会话按 executing-plans 批量执行并设检查点  

**Which approach?**
