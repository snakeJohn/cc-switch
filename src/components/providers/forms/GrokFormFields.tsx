import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Info, Loader2, LogOut, RefreshCw, User } from "lucide-react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "sonner";
import { ApiKeySection, EndpointField } from "./shared";
import {
  GROK_API_BACKENDS,
  type GrokApiBackend,
} from "@/config/grokProviderPresets";
import type { ProviderCategory } from "@/types";
import {
  getGrokAuthStatus,
  logoutGrokAccounts,
  removeGrokAccount,
  setActiveGrokAccount,
  type GrokAuthStatus,
} from "@/lib/api/config";
import { copyText } from "@/lib/clipboard";
import { extractErrorMessage } from "@/utils/errorUtils";

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
  const [authStatus, setAuthStatus] = useState<GrokAuthStatus | null>(null);
  const [authLoading, setAuthLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState(false);

  const showOfficial = isOfficial || category === "official";

  const refreshAuth = useCallback(async () => {
    setAuthLoading(true);
    try {
      const status = await getGrokAuthStatus();
      setAuthStatus(status);
    } catch {
      setAuthStatus(null);
    } finally {
      setAuthLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!showOfficial) {
      setAuthStatus(null);
      return;
    }
    void refreshAuth();
  }, [showOfficial, refreshAuth]);

  const handleSwitchAccount = async (accountId: string) => {
    setActionLoading(true);
    try {
      const status = await setActiveGrokAccount(accountId);
      setAuthStatus(status);
      toast.success(
        t("providerForm.grokAccountSwitched", {
          defaultValue: "已切换活跃账号",
        }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("providerForm.grokAccountSwitchFailed", {
            defaultValue: "切换账号失败",
          }),
      );
    } finally {
      setActionLoading(false);
    }
  };

  const handleRemoveAccount = async (accountId: string) => {
    setActionLoading(true);
    try {
      const status = await removeGrokAccount(accountId);
      setAuthStatus(status);
      toast.success(
        t("providerForm.grokAccountRemoved", {
          defaultValue: "账号已移除",
        }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("providerForm.grokAccountRemoveFailed", {
            defaultValue: "移除账号失败",
          }),
      );
    } finally {
      setActionLoading(false);
    }
  };

  const handleLogoutAll = async () => {
    setActionLoading(true);
    try {
      const status = await logoutGrokAccounts();
      setAuthStatus(status);
      toast.success(
        t("providerForm.grokLoggedOut", {
          defaultValue: "已退出官方登录",
        }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("providerForm.grokLogoutFailed", {
            defaultValue: "退出登录失败",
          }),
      );
    } finally {
      setActionLoading(false);
    }
  };

  const handleCopyLoginCommand = async () => {
    try {
      await copyText("grok login");
      toast.success(
        t("providerForm.grokLoginCopied", {
          defaultValue: "已复制：grok login",
        }),
      );
    } catch {
      toast.error(
        t("common.copyFailed", { defaultValue: "复制失败" }),
      );
    }
  };

  const handleCopyAuthPath = async () => {
    if (!authStatus?.authPath) return;
    try {
      await copyText(authStatus.authPath);
      toast.success(
        t("providerForm.grokAuthPathCopied", {
          defaultValue: "已复制 auth.json 路径",
        }),
      );
    } catch {
      toast.error(t("common.copyFailed", { defaultValue: "复制失败" }));
    }
  };

  if (showOfficial) {
    const authenticated = authStatus?.authenticated ?? false;
    const accounts = authStatus?.accounts ?? [];
    const activeId =
      authStatus?.activeAccountId ??
      accounts.find((a) => a.isActive)?.id ??
      null;

    return (
      <div className="space-y-3">
        <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
            <div className="space-y-3 flex-1 min-w-0">
              <div className="flex items-center justify-between gap-2">
                <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
                  {t("providerForm.grokOfficialNoApiKey", {
                    defaultValue:
                      "Official does not require API Key; run grok login locally",
                  })}
                </p>
                {authLoading ? (
                  <Loader2 className="h-4 w-4 animate-spin text-blue-600" />
                ) : (
                  <Badge
                    variant={authenticated ? "default" : "secondary"}
                    className={
                      authenticated
                        ? "bg-green-500 hover:bg-green-600 shrink-0"
                        : "shrink-0"
                    }
                  >
                    {authenticated
                      ? t("providerForm.grokAuthLoggedIn", {
                          defaultValue: "Logged in",
                        })
                      : t("providerForm.grokAuthNotLoggedIn", {
                          defaultValue: "Not logged in",
                        })}
                  </Badge>
                )}
              </div>

              {authenticated && accounts.length > 0 ? (
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <User className="h-3.5 w-3.5 text-blue-700 dark:text-blue-300" />
                    <span className="text-xs font-medium text-blue-800 dark:text-blue-200">
                      {t("providerForm.grokActiveAccount", {
                        defaultValue: "当前账号",
                      })}
                    </span>
                  </div>
                  {accounts.length === 1 ? (
                    <p className="text-xs text-blue-700 dark:text-blue-300 truncate">
                      {accounts[0].email || accounts[0].id}
                      {accounts[0].expiresAt
                        ? ` · exp ${accounts[0].expiresAt}`
                        : ""}
                    </p>
                  ) : (
                    <Select
                      value={activeId ?? undefined}
                      onValueChange={(v) => void handleSwitchAccount(v)}
                      disabled={actionLoading}
                    >
                      <SelectTrigger className="h-8 bg-white/70 dark:bg-black/20">
                        <SelectValue
                          placeholder={t("providerForm.grokSelectAccount", {
                            defaultValue: "选择账号",
                          })}
                        />
                      </SelectTrigger>
                      <SelectContent>
                        {accounts.map((account) => (
                          <SelectItem key={account.id} value={account.id}>
                            {account.email || account.id}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                </div>
              ) : (
                <p className="text-xs text-blue-700 dark:text-blue-300">
                  {t("providerForm.grokLoginHint", {
                    defaultValue: "请运行 grok login",
                  })}
                </p>
              )}

              <p className="text-xs text-blue-700 dark:text-blue-300">
                {t("providerForm.officialHint", {
                  defaultValue:
                    "💡 Official provider uses browser login, no API Key needed",
                })}
              </p>

              <div className="flex flex-wrap gap-2 pt-1">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  disabled={actionLoading}
                  onClick={() => void handleCopyLoginCommand()}
                >
                  {t("providerForm.grokSwitchLogin", {
                    defaultValue: "切换/重新登录",
                  })}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="h-7 text-xs"
                  disabled={authLoading || actionLoading}
                  onClick={() => void refreshAuth()}
                >
                  <RefreshCw className="h-3.5 w-3.5 mr-1" />
                  {t("common.refresh", { defaultValue: "刷新" })}
                </Button>
                {authenticated && activeId ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-7 text-xs text-destructive hover:text-destructive"
                    disabled={actionLoading}
                    onClick={() => void handleRemoveAccount(activeId)}
                  >
                    <LogOut className="h-3.5 w-3.5 mr-1" />
                    {t("providerForm.grokLogout", {
                      defaultValue: "退出当前",
                    })}
                  </Button>
                ) : null}
                {authenticated && accounts.length > 1 ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-7 text-xs text-destructive hover:text-destructive"
                    disabled={actionLoading}
                    onClick={() => void handleLogoutAll()}
                  >
                    {t("providerForm.grokLogoutAll", {
                      defaultValue: "退出全部",
                    })}
                  </Button>
                ) : null}
                {authStatus?.authPath ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-7 text-xs"
                    onClick={() => void handleCopyAuthPath()}
                  >
                    {t("providerForm.grokCopyAuthPath", {
                      defaultValue: "复制 auth 路径",
                    })}
                  </Button>
                ) : null}
              </div>
              <p className="text-[11px] text-blue-600/80 dark:text-blue-300/80">
                {t("providerForm.grokLoginSwitchHint", {
                  defaultValue:
                    "复制后在终端执行 grok login 可切换到其他账号；若 auth.json 中有多个条目，可在上方下拉框切换活跃账号。",
                })}
              </p>
            </div>
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
