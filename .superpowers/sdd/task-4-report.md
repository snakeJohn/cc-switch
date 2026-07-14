# Task 4 Report: 前端 AppId + appConfig + i18n

**Status:** Complete  
**Date:** 2026-07-14  
**Commit message:** `feat(grok): register frontend AppId and settings dir override`

## Summary

Registered frontend `AppId` `"grok"` (UI label **Grok Build**), app switcher/config icons, settings config-dir override (mirror of Hermes), and minimal i18n. MCP/Skills app lists intentionally **not** updated (P1 constraint; Tasks 7–8).

## Changes

### Types & config
| File | Change |
|------|--------|
| `src/lib/api/types.ts` | `AppId` includes `"grok"` |
| `src/config/appConfig.tsx` | `APP_IDS` + `APP_ICON_MAP.grok` (xai icon, neutral theme). **No** `MCP_APP_IDS` / `SKILLS_APP_IDS` |
| `src/types.ts` | `VisibleApps.grok`, `Settings.grokConfigDir`, optional `McpApps.grok` |
| `src/lib/api/skills.ts` | `AppType` + optional `SkillApps.grok` |
| `src/types/proxy.ts` | optional `ProxyTakeoverStatus.grok` |
| `src/lib/schemas/settings.ts` | `hermesConfigDir` + `grokConfigDir` |

### Settings directory override
| File | Change |
|------|--------|
| `src/hooks/useDirectorySettings.ts` | Meta/defaults/load/reset for `grok` → `~/.grok` / `grokConfigDir` |
| `src/hooks/useSettings.ts` | Reset includes `grok` override |
| `src/components/settings/DirectorySettings.tsx` | Directory input row |
| `src/components/settings/SettingsPage.tsx` | Passes `grokDir` |
| `src/components/settings/AppVisibilitySettings.tsx` | Visibility toggle for Grok Build |

### App surface (exhaustive `AppId`)
| File | Change |
|------|--------|
| `src/App.tsx` | `VALID_APPS`, default `visibleApps`, `getFirstVisibleApp` |
| `src/components/AppSwitcher.tsx` | List + icon/name maps |
| `src/components/prompts/PromptFormPanel.tsx` / `PromptFormModal.tsx` | `filenameMap.grok` |
| `src/components/providers/forms/EndpointSpeedTest.tsx` | Timeout map |
| `src/components/mcp/UnifiedMcpPanel.tsx` / `skills/UnifiedSkillsPanel.tsx` | Count keys for indexing (still not in MCP/Skills panels) |

### i18n
- `zh.json` / `en.json` / `ja.json` / `zh-TW.json`
- Keys: `apps.grok`, `settings.grokConfigDir*`, `settings.browsePlaceholderGrok`, `provider.grokOfficialNoApiKey`, `provider.grokApiHint`

### Tests
- `tests/msw/state.ts`, `tests/hooks/useDirectorySettings.test.tsx`, `tests/hooks/useSettings.test.tsx`

## Verification

```text
node node_modules/typescript/bin/tsc --noEmit
→ exit 0
```

(`pnpm exec tsc` may re-run install depending on env; direct `tsc` used after deps present.)

## Out of scope (by design)

- `GrokFormFields` / presets (Task 5)
- MCP / Skills for grok (Tasks 7–8)
- Full provider form / live-sync UI beyond app registration

## Concerns

1. Selecting Grok Build works, but provider form/presets are not fully wired until Task 5 — empty or generic form may appear.
2. `MCP_APP_IDS` / `SKILLS_APP_IDS` still exclude `grok`; count maps only hold a zero key for type indexing.
3. Accidental `pnpm-workspace.yaml` install side-effect was reverted and not committed.
