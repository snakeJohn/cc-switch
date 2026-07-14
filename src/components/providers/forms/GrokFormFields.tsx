import { useTranslation } from "react-i18next";
import { Info } from "lucide-react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiKeySection, EndpointField } from "./shared";
import {
  GROK_API_BACKENDS,
  type GrokApiBackend,
} from "@/config/grokProviderPresets";
import type { ProviderCategory } from "@/types";

interface GrokFormFieldsProps {
  isOfficial: boolean;
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  model: string;
  onModelChange: (value: string) => void;
  apiBackend: GrokApiBackend;
  onApiBackendChange: (value: GrokApiBackend) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
}

export function GrokFormFields({
  isOfficial,
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  model,
  onModelChange,
  apiBackend,
  onApiBackendChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
}: GrokFormFieldsProps) {
  const { t } = useTranslation();

  if (isOfficial || category === "official") {
    return (
      <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
        <div className="flex gap-3">
          <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
          <div className="space-y-1">
            <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
              {t("providerForm.grokOfficialNoApiKey", {
                defaultValue:
                  "Official does not require API Key; run grok login locally",
              })}
            </p>
            <p className="text-xs text-blue-700 dark:text-blue-300">
              {t("providerForm.officialHint", {
                defaultValue:
                  "💡 Official provider uses browser login, no API Key needed",
              })}
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <>
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <EndpointField
        id="grok-baseurl"
        label={t("providerForm.apiEndpoint", {
          defaultValue: "API Endpoint",
        })}
        value={baseUrl}
        onChange={onBaseUrlChange}
        placeholder="https://api.x.ai/v1"
        hint={t("providerForm.grokApiHint", {
          defaultValue:
            "Fill in an OpenAI-compatible endpoint; api_backend may be chat_completions / responses / messages",
        })}
        showManageButton={false}
      />

      <div className="space-y-2">
        <FormLabel htmlFor="grok-model">
          {t("providerForm.mainModel", { defaultValue: "Model" })}
        </FormLabel>
        <Input
          id="grok-model"
          value={model}
          onChange={(e) => onModelChange(e.target.value)}
          placeholder="grok-build"
        />
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="grok-api-backend">
          {t("providerForm.grokApiBackendLabel", {
            defaultValue: "API Backend",
          })}
        </FormLabel>
        <Select
          value={apiBackend}
          onValueChange={(v) => onApiBackendChange(v as GrokApiBackend)}
        >
          <SelectTrigger id="grok-api-backend">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {GROK_API_BACKENDS.map((backend) => (
              <SelectItem key={backend.value} value={backend.value}>
                {t(backend.labelKey, { defaultValue: backend.value })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("providerForm.grokApiHint", {
            defaultValue:
              "Fill in an OpenAI-compatible endpoint; api_backend may be chat_completions / responses / messages",
          })}
        </p>
      </div>
    </>
  );
}
