/**
 * Grok Build provider presets.
 * settings_config shape: { auth, config (toml string), meta }
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme } from "./claudeProviderPresets";

export type GrokApiBackend =
  | "chat_completions"
  | "responses"
  | "messages";

export const GROK_API_BACKENDS: Array<{
  value: GrokApiBackend;
  labelKey: string;
}> = [
  {
    value: "chat_completions",
    labelKey: "providerForm.grokApiBackend.chatCompletions",
  },
  {
    value: "responses",
    labelKey: "providerForm.grokApiBackend.responses",
  },
  {
    value: "messages",
    labelKey: "providerForm.grokApiBackend.messages",
  },
];

export interface GrokProviderSettingsConfig {
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
}

export interface GrokProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: GrokProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  icon?: string;
  iconColor?: string;
  endpointCandidates?: string[];
  /** Optional visual theme used by preset selector cards */
  theme?: PresetTheme;
}

export function generateGrokOfficialConfig(
  model = "grok-build",
): GrokProviderSettingsConfig {
  return {
    auth: {},
    config: `[models]\ndefault = ${JSON.stringify(model)}\n`,
    meta: {
      isOfficial: true,
      model,
      apiBackend: "responses",
    },
  };
}

export function generateGrokThirdPartyConfig(opts: {
  displayName: string;
  model: string;
  baseUrl: string;
  apiKey: string;
  apiBackend: GrokApiBackend;
}): GrokProviderSettingsConfig {
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

export const GROK_DEFAULT_SETTINGS = generateGrokThirdPartyConfig({
  displayName: "Custom",
  model: "grok-build",
  baseUrl: "https://api.x.ai/v1",
  apiKey: "",
  apiBackend: "chat_completions",
});

export const GROK_DEFAULT_CONFIG = JSON.stringify(
  GROK_DEFAULT_SETTINGS,
  null,
  2,
);

export const grokProviderPresets: GrokProviderPreset[] = [
  {
    name: "xAI Official",
    nameKey: "providerForm.presets.grokOfficial",
    websiteUrl: "https://grok.com",
    isOfficial: true,
    category: "official",
    icon: "xai",
    settingsConfig: generateGrokOfficialConfig("grok-build"),
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
    endpointCandidates: ["https://api.x.ai/v1"],
  },
];
