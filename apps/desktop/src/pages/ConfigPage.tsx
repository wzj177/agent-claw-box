import { useEffect, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { ArrowLeft, Save, RefreshCw, Eye, EyeOff, ExternalLink } from "lucide-react";
import { open } from "@tauri-apps/plugin-shell";
import {
  api,
  type AgentInfo,
  type TemplateInfo,
  type AgentConfigEntry,
  type ConfigField,
} from "../lib/api";

function normalizeModelValue(provider: string, rawValue: string): string {
  const value = rawValue.trim();
  if (!value) return value;

  if (provider === "qwen") {
    if (value.startsWith("qwen-api/")) return value;
    if (value.startsWith("qwen/")) return `qwen-api/${value.slice("qwen/".length)}`;
    if (value.startsWith("qwen-portal/")) return `qwen-api/${value.slice("qwen-portal/".length)}`;
    if (!value.includes("/")) return `qwen-api/${value}`;
  }

  if (provider === "openrouter") {
    if (value.startsWith("openrouter/")) return value;
    return `openrouter/${value}`;
  }

  return value;
}

export function ConfigPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [template, setTemplate] = useState<TemplateInfo | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [savedValues, setSavedValues] = useState<Record<string, string>>({});
  const [revealSecrets, setRevealSecrets] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    const load = async () => {
      try {
        // Load agent info
        const agents = await api.listAgents();
        const a = agents.find((x) => x.id === id);
        if (!a) {
          setError("未找到该实例");
          return;
        }
        setAgent(a);

        // Load template schema
        const tmpls = await api.listTemplates();
        const t = tmpls.find((x) => x.id === a.template);
        setTemplate(t ?? null);

        // Load saved config
        const configs = await api.getAgentConfig(id);
        const map: Record<string, string> = {};
        for (const c of configs) {
          map[c.config_key] = c.config_value;
        }
        setSavedValues(map);

        // Initialize form values: saved values > defaults
        const formValues: Record<string, string> = {};
        if (t) {
          for (const field of t.config_schema) {
            formValues[field.key] = map[field.key] ?? field.default ?? "";
          }
        }
        // Include any saved values not in schema
        for (const key of Object.keys(map)) {
          if (!(key in formValues)) {
            formValues[key] = map[key];
          }
        }

        if (a.template === "openclaw") {
          const provider = formValues["llm_provider"] ?? "";
          const modelValue = formValues["model"] ?? "";
          formValues["model"] = normalizeModelValue(provider, modelValue);
        }

        setValues(formValues);
      } catch (e) {
        setError(String(e));
      }
    };
    load();
  }, [id]);

  const handleSave = async () => {
    if (!id || !template) return;
    setSaving(true);
    setError(null);
    setSuccess(null);
    try {
      const entries: AgentConfigEntry[] = template.config_schema.map((field) => ({
        config_key: field.key,
        config_value:
          agent?.template === "openclaw" && field.key === "model"
            ? normalizeModelValue(values["llm_provider"] ?? "", values[field.key] ?? "")
            : values[field.key] ?? "",
        is_secret: field.type === "secret",
      }));
      await api.saveAgentConfig(id, entries);
      const normalizedValues = { ...values };
      if (agent?.template === "openclaw") {
        normalizedValues["model"] = normalizeModelValue(
          values["llm_provider"] ?? "",
          values["model"] ?? ""
        );
        setValues(normalizedValues);
      }
      setSavedValues(normalizedValues);
      setSuccess("配置已保存");
    } catch (e) {
      setError(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const handleApply = async () => {
    if (!id) return;
    setApplying(true);
    setError(null);
    setSuccess(null);
    try {
      await handleSave();
      await api.applyAgentConfig(id);
      setSuccess("配置已应用，容器已重启");
    } catch (e) {
      setError(`应用配置失败: ${e}`);
    } finally {
      setApplying(false);
    }
  };

  const hasChanges = template?.config_schema.some(
    (f) => (values[f.key] ?? "") !== (savedValues[f.key] ?? f.default ?? "")
  );

  const toggleReveal = (key: string) => {
    setRevealSecrets((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  if (!agent) {
    return (
      <div className="p-6">
        {error ? (
          <div className="text-red-600">{error}</div>
        ) : (
          <div className="text-neutral-400">加载中…</div>
        )}
      </div>
    );
  }

  return (
    <div className="p-6 max-w-2xl">
      {/* Header */}
      <div className="flex items-center gap-3 mb-6">
        <button onClick={() => navigate("/")} className="btn-text" title="返回">
          <ArrowLeft className="w-4 h-4" />
        </button>
        <div>
          <h1 className="text-page-title text-neutral-800">{agent.name} — 配置</h1>
          <p className="text-caption text-neutral-400">
            {agent.template} · 实例 #{agent.instance_no} · v{agent.version}
          </p>
        </div>
      </div>

      {/* Messages */}
      {error && (
        <div className="mb-4 px-4 py-3 rounded-md bg-red-50 text-red-600 text-caption">
          {error}
        </div>
      )}
      {success && (
        <div className="mb-4 px-4 py-3 rounded-md bg-green-50 text-green-600 text-caption">
          {success}
        </div>
      )}

      {/* Config form */}
      {template && template.config_schema.length > 0 ? (
        <div className="bg-white rounded-card border border-neutral-200 shadow-card">
          <div className="px-5 py-4 border-b border-neutral-100">
            <h2 className="text-section-title text-neutral-800">基础配置</h2>
            <p className="text-caption text-neutral-400 mt-1">
              配置完成后点击"保存并应用"以生效，容器将自动重启
            </p>
          </div>

          <div className="px-5 py-4 space-y-4">
            {template.config_schema.map((field) => (
              <ConfigFieldInput
                key={field.key}
                field={field}
                value={values[field.key] ?? ""}
                onChange={(v) => {
                  setValues((prev) => {
                    const next = { ...prev, [field.key]: v };
                    // When provider changes, sync model to the new provider's default
                    if (field.key === "llm_provider" && PROVIDER_INFO[v]) {
                      next["model"] = PROVIDER_INFO[v].defaultModel;
                    }
                    return next;
                  });
                }}
                revealed={revealSecrets.has(field.key)}
                onToggleReveal={() => toggleReveal(field.key)}
                selectedProvider={values["llm_provider"] ?? ""}
                templateId={agent.template}
              />
            ))}
          </div>

          {/* Actions */}
          <div className="px-5 py-4 flex items-center gap-3 border-t border-neutral-100">
            <button onClick={handleSave} disabled={saving || !hasChanges} className="btn-default">
              <Save className="w-3.5 h-3.5" />
              {saving ? "保存中…" : "仅保存"}
            </button>
            <button onClick={handleApply} disabled={applying} className="btn-primary">
              <RefreshCw className={`w-3.5 h-3.5 ${applying ? "animate-spin" : ""}`} />
              {applying ? "应用中…" : "保存并应用"}
            </button>
          </div>
        </div>
      ) : (
        <div className="bg-white rounded-card border border-neutral-200 shadow-card px-5 py-8 text-center">
          <p className="text-body text-neutral-500">该模板暂无可配置项</p>
          <p className="text-caption text-neutral-400 mt-1">
            你可以通过终端进入容器手动配置
          </p>
        </div>
      )}

      {/* Quick actions */}
      <div className="mt-4 flex items-center gap-3">
        <button
          onClick={() => api.openAgentShell(agent.id)}
          className="btn-default"
        >
          打开终端
        </button>
        <button
          onClick={() => api.openAgentBrowser(agent.id)}
          className="btn-default"
        >
          打开 Web UI
        </button>
      </div>
    </div>
  );
}

// Map llm_provider to the env var name, default model, hint, and API key URL
const PROVIDER_INFO: Record<string, { envVar: string; defaultModel: string; hint: string; url: string }> = {
  anthropic: { envVar: "ANTHROPIC_API_KEY", defaultModel: "anthropic/claude-sonnet-4-20250514", hint: "前往 Anthropic 控制台获取", url: "https://console.anthropic.com/settings/keys" },
  openai: { envVar: "OPENAI_API_KEY", defaultModel: "openai/gpt-4o", hint: "前往 OpenAI 平台获取", url: "https://platform.openai.com/api-keys" },
  deepseek: { envVar: "DEEPSEEK_API_KEY", defaultModel: "deepseek/deepseek-chat", hint: "前往 DeepSeek 平台获取", url: "https://platform.deepseek.com/api_keys" },
  ollama: { envVar: "（本地模型无需 Key）", defaultModel: "ollama/llama3", hint: "需本地运行 Ollama 服务", url: "https://ollama.com/download" },
  openrouter: { envVar: "OPENROUTER_API_KEY", defaultModel: "openrouter/anthropic/claude-sonnet-4-20250514", hint: "前往 OpenRouter 获取", url: "https://openrouter.ai/keys" },
  mistral: { envVar: "MISTRAL_API_KEY", defaultModel: "mistral/mistral-large-latest", hint: "前往 Mistral 控制台获取", url: "https://console.mistral.ai/api-keys" },
  moonshot: { envVar: "MOONSHOT_API_KEY", defaultModel: "moonshot/moonshot-v1-8k", hint: "前往 Moonshot 平台获取", url: "https://platform.moonshot.cn/console/api-keys" },
  qwen: { envVar: "QWEN_API_KEY", defaultModel: "qwen-api/qwen-plus", hint: "前往阿里云百炼获取", url: "https://dashscope.console.aliyun.com/apiKey" },
};

function ConfigFieldInput({
  field,
  value,
  onChange,
  revealed,
  onToggleReveal,
  selectedProvider,
  templateId,
}: {
  field: ConfigField;
  value: string;
  onChange: (v: string) => void;
  revealed: boolean;
  onToggleReveal: () => void;
  selectedProvider: string;
  templateId: string;
}) {
  const isOpenClaw = templateId === "openclaw";
  const providerMeta = isOpenClaw ? PROVIDER_INFO[selectedProvider] : undefined;

  // Dynamic hints for OpenClaw api_key field
  let dynamicEnvHint = field.env_name;
  let dynamicPlaceholder = field.default ? `默认: ${field.default}` : `请输入${field.label}`;
  let helperText: string | null = null;

  if (isOpenClaw && field.key === "api_key" && providerMeta) {
    dynamicEnvHint = providerMeta.envVar;
    dynamicPlaceholder = providerMeta.hint;
    if (selectedProvider === "ollama") {
      helperText = "Ollama 为本地模型，API Key 可留空";
    } else {
      helperText = providerMeta.hint;
    }
  }
  if (isOpenClaw && field.key === "model" && providerMeta) {
    dynamicPlaceholder = `默认: ${providerMeta.defaultModel}`;
    helperText = "格式: provider/model-name，留空使用默认值";
  }

  const inputBase =
    "w-full px-3 py-2 text-body bg-white border border-neutral-300 rounded-md " +
    "placeholder:text-neutral-400 " +
    "focus:outline-none focus:border-primary-400 focus:ring-1 focus:ring-primary-100 " +
    "transition-colors duration-150";

  return (
    <div>
      <label className="block text-body font-medium text-neutral-700 mb-1">
        {field.label}
        {field.required && selectedProvider !== "ollama" && field.key !== "model" && (
          <span className="text-red-500 ml-0.5">*</span>
        )}
        {dynamicEnvHint && (
          <span className="text-caption text-neutral-400 font-normal ml-2">
            → {dynamicEnvHint}
          </span>
        )}
      </label>

      {field.type === "select" ? (
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={inputBase}
        >
          <option value="">请选择…</option>
          {field.options.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      ) : field.type === "secret" ? (
        <div className="relative">
          <input
            type={revealed ? "text" : "password"}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={dynamicPlaceholder}
            className={inputBase + " pr-10"}
          />
          <button
            type="button"
            onClick={onToggleReveal}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600"
          >
            {revealed ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
          </button>
        </div>
      ) : (
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={dynamicPlaceholder}
          className={inputBase}
        />
      )}
      {helperText && (
        <p className="mt-1 text-caption text-neutral-400 flex items-center gap-1">
          {helperText}
          {isOpenClaw && providerMeta?.url && (
            <button
              type="button"
              onClick={() => open(providerMeta.url)}
              className="inline-flex items-center gap-0.5 text-primary-500 hover:text-primary-600 transition-colors"
              title={`打开 ${providerMeta.url}`}
            >
              <ExternalLink className="w-3 h-3" />
              <span className="underline">获取</span>
            </button>
          )}
        </p>
      )}
    </div>
  );
}
