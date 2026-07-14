import { useState, useCallback } from "react";
import type { AppId } from "@/lib/api";
import {
  generateGrokOfficialConfig,
  generateGrokThirdPartyConfig,
  GROK_DEFAULT_CONFIG,
  type GrokApiBackend,
  type GrokProviderSettingsConfig,
} from "@/config/grokProviderPresets";

interface UseGrokFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    name?: string;
  };
  appId: AppId;
  onSettingsConfigChange: (config: string) => void;
}

function parseGrokSettings(
  settingsConfig?: Record<string, unknown>,
): GrokProviderSettingsConfig | null {
  if (!settingsConfig || typeof settingsConfig !== "object") return null;
  const meta = (settingsConfig.meta ?? {}) as GrokProviderSettingsConfig["meta"];
  return {
    auth:
      typeof settingsConfig.auth === "object" && settingsConfig.auth !== null
        ? (settingsConfig.auth as Record<string, unknown>)
        : {},
    config:
      typeof settingsConfig.config === "string" ? settingsConfig.config : "",
    meta,
  };
}

export interface GrokFormState {
  isOfficial: boolean;
  baseUrl: string;
  apiKey: string;
  model: string;
  apiBackend: GrokApiBackend;
  displayName: string;
  handleBaseUrlChange: (baseUrl: string) => void;
  handleApiKeyChange: (apiKey: string) => void;
  handleModelChange: (model: string) => void;
  handleApiBackendChange: (backend: GrokApiBackend) => void;
  handleDisplayNameChange: (name: string) => void;
  resetGrokState: (config?: GrokProviderSettingsConfig) => void;
}

export function useGrokFormState({
  initialData,
  appId,
  onSettingsConfigChange,
}: UseGrokFormStateParams): GrokFormState {
  const parsed = parseGrokSettings(initialData?.settingsConfig);
  const initialMeta = parsed?.meta ?? {};
  const initialIsOfficial = Boolean(initialMeta.isOfficial);

  const [isOfficial, setIsOfficial] = useState<boolean>(() => {
    if (appId !== "grok") return false;
    return initialIsOfficial;
  });

  const [baseUrl, setBaseUrl] = useState<string>(() => {
    if (appId !== "grok") return "";
    return initialMeta.baseUrl ?? "https://api.x.ai/v1";
  });

  const [apiKey, setApiKey] = useState<string>(() => {
    if (appId !== "grok") return "";
    return initialMeta.apiKey ?? "";
  });

  const [model, setModel] = useState<string>(() => {
    if (appId !== "grok") return "grok-build";
    return initialMeta.model ?? "grok-build";
  });

  const [apiBackend, setApiBackend] = useState<GrokApiBackend>(() => {
    if (appId !== "grok") return "chat_completions";
    return initialMeta.apiBackend ?? "chat_completions";
  });

  const [displayName, setDisplayName] = useState<string>(() => {
    if (appId !== "grok") return "Custom";
    return (
      initialMeta.displayName ||
      initialData?.name ||
      (initialIsOfficial ? "xAI Official" : "Custom")
    );
  });

  const emitConfig = useCallback(
    (next: {
      isOfficial: boolean;
      baseUrl: string;
      apiKey: string;
      model: string;
      apiBackend: GrokApiBackend;
      displayName: string;
    }) => {
      const settings: GrokProviderSettingsConfig = next.isOfficial
        ? generateGrokOfficialConfig(next.model || "grok-build")
        : generateGrokThirdPartyConfig({
            displayName: next.displayName || next.model || "Custom",
            model: next.model || "grok-build",
            baseUrl: next.baseUrl.trim().replace(/\/+$/, ""),
            apiKey: next.apiKey,
            apiBackend: next.apiBackend || "chat_completions",
          });
      onSettingsConfigChange(JSON.stringify(settings, null, 2));
    },
    [onSettingsConfigChange],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      setBaseUrl(value);
      emitConfig({
        isOfficial,
        baseUrl: value,
        apiKey,
        model,
        apiBackend,
        displayName,
      });
    },
    [apiBackend, apiKey, displayName, emitConfig, isOfficial, model],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      setApiKey(value);
      emitConfig({
        isOfficial,
        baseUrl,
        apiKey: value,
        model,
        apiBackend,
        displayName,
      });
    },
    [apiBackend, baseUrl, displayName, emitConfig, isOfficial, model],
  );

  const handleModelChange = useCallback(
    (value: string) => {
      setModel(value);
      emitConfig({
        isOfficial,
        baseUrl,
        apiKey,
        model: value,
        apiBackend,
        displayName,
      });
    },
    [apiBackend, apiKey, baseUrl, displayName, emitConfig, isOfficial],
  );

  const handleApiBackendChange = useCallback(
    (value: GrokApiBackend) => {
      setApiBackend(value);
      emitConfig({
        isOfficial,
        baseUrl,
        apiKey,
        model,
        apiBackend: value,
        displayName,
      });
    },
    [apiKey, baseUrl, displayName, emitConfig, isOfficial, model],
  );

  const handleDisplayNameChange = useCallback(
    (value: string) => {
      setDisplayName(value);
      emitConfig({
        isOfficial,
        baseUrl,
        apiKey,
        model,
        apiBackend,
        displayName: value,
      });
    },
    [apiBackend, apiKey, baseUrl, emitConfig, isOfficial, model],
  );

  const resetGrokState = useCallback(
    (config?: GrokProviderSettingsConfig) => {
      if (!config) {
        setIsOfficial(false);
        setBaseUrl("https://api.x.ai/v1");
        setApiKey("");
        setModel("grok-build");
        setApiBackend("chat_completions");
        setDisplayName("Custom");
        onSettingsConfigChange(GROK_DEFAULT_CONFIG);
        return;
      }

      const nextIsOfficial = Boolean(config.meta?.isOfficial);
      const nextBaseUrl = config.meta?.baseUrl ?? "https://api.x.ai/v1";
      const nextApiKey = config.meta?.apiKey ?? "";
      const nextModel = config.meta?.model ?? "grok-build";
      const nextBackend: GrokApiBackend =
        config.meta?.apiBackend ??
        (nextIsOfficial ? "responses" : "chat_completions");
      const nextDisplayName =
        config.meta?.displayName ||
        (nextIsOfficial ? "xAI Official" : "Custom");

      setIsOfficial(nextIsOfficial);
      setBaseUrl(nextBaseUrl);
      setApiKey(nextApiKey);
      setModel(nextModel);
      setApiBackend(nextBackend);
      setDisplayName(nextDisplayName);
      onSettingsConfigChange(JSON.stringify(config, null, 2));
    },
    [onSettingsConfigChange],
  );

  return {
    isOfficial,
    baseUrl,
    apiKey,
    model,
    apiBackend,
    displayName,
    handleBaseUrlChange,
    handleApiKeyChange,
    handleModelChange,
    handleApiBackendChange,
    handleDisplayNameChange,
    resetGrokState,
  };
}
