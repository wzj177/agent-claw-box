import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Loader2, RotateCcw, Save, Settings2, Shield, Trash2 } from "lucide-react";
import { api, type AppSettings, type ProxyPreview } from "../lib/api";

const DEFAULT_SETTINGS: AppSettings = {
  instance_autostart_enabled: true,
  instance_autostart_delay_secs: 8,
  proxy_enabled: false,
  proxy_url: "",
  no_proxy: "",
};

export function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [proxyPreview, setProxyPreview] = useState<ProxyPreview | null>(null);
  const [appAutostart, setAppAutostart] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [clearingLogs, setClearingLogs] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    const proxyUrl = settings.proxy_url?.trim();
    if (!settings.proxy_enabled || !proxyUrl) {
      setProxyPreview(null);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(async () => {
      try {
        const preview = await api.getProxyPreview(proxyUrl);
        if (!cancelled) {
          setProxyPreview(preview);
        }
      } catch {
        if (!cancelled) {
          setProxyPreview(null);
        }
      }
    }, 180);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [settings.proxy_enabled, settings.proxy_url]);

  async function load() {
    setLoading(true);
    setMessage(null);
    try {
      const [savedSettings, autostartEnabled] = await Promise.all([
        api.getAppSettings(),
        isEnabled(),
      ]);
      setSettings({
        ...savedSettings,
        proxy_url: savedSettings.proxy_url ?? "",
        no_proxy: savedSettings.no_proxy ?? "",
      });
      setAppAutostart(autostartEnabled);
    } catch (error) {
      setMessage({ ok: false, text: `加载设置失败: ${error}` });
    } finally {
      setLoading(false);
    }
  }

  function updateSettings<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    setSettings((current) => ({ ...current, [key]: value }));
  }

  async function handleSave() {
    setSaving(true);
    setMessage(null);
    try {
      if (appAutostart) {
        await enable();
      } else {
        await disable();
      }

      await api.saveAppSettings({
        ...settings,
        instance_autostart_delay_secs: Math.max(0, Number(settings.instance_autostart_delay_secs) || 0),
        proxy_url: settings.proxy_url?.trim() ? settings.proxy_url.trim() : null,
        no_proxy: settings.no_proxy?.trim() ? settings.no_proxy.trim() : null,
      });

      setMessage({ ok: true, text: "设置已保存" });
    } catch (error) {
      setMessage({ ok: false, text: `保存失败: ${error}` });
    } finally {
      setSaving(false);
    }
  }

  async function handleClearLogs() {
    setClearingLogs(true);
    setMessage(null);
    try {
      const result = await api.clearLocalLogs();
      setMessage({
        ok: true,
        text: `已清理 ${result.removed_native_logs} 个本地日志文件、${result.removed_pid_files} 个 PID 文件，并删除 ${result.removed_metrics_rows} 条监控记录`,
      });
    } catch (error) {
      setMessage({ ok: false, text: `清理失败: ${error}` });
    } finally {
      setClearingLogs(false);
    }
  }

  return (
    <div className="min-h-full bg-neutral-50 px-6 py-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h1 className="text-page-title text-neutral-800">应用设置</h1>
            <p className="mt-1 text-sm text-neutral-500">
              管理桌面应用启动行为、实例延迟启动、本地日志清理和网络代理。
            </p>
          </div>
          <button onClick={handleSave} className="btn-primary" disabled={loading || saving}>
            {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
            <span>保存设置</span>
          </button>
        </div>

        {message && (
          <div
            className={`rounded-xl border px-4 py-3 text-sm ${
              message.ok
                ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                : "border-red-200 bg-red-50 text-red-600"
            }`}
          >
            {message.text}
          </div>
        )}

        <section className="rounded-xl border border-neutral-200 bg-white p-6">
          <div className="mb-5 flex items-center gap-2 text-neutral-800">
            <Settings2 className="h-4 w-4 text-primary-500" />
            <h2 className="text-base font-semibold">启动设置</h2>
          </div>

          <div className="space-y-5">
            <ToggleRow
              title="开机启动应用"
              description="登录系统后自动启动 AgentBox，并按最小化参数进入后台。"
              checked={appAutostart}
              disabled={loading || saving}
              onChange={setAppAutostart}
            />

            <ToggleRow
              title="开机自动启动实例"
              description="应用启动后遍历已勾选自动启动的实例，按顺序逐个启动。"
              checked={settings.instance_autostart_enabled}
              disabled={loading || saving}
              onChange={(checked) => updateSettings("instance_autostart_enabled", checked)}
            />

            <div className="rounded-lg border border-neutral-100 bg-neutral-50 px-4 py-4">
              <label className="mb-2 block text-sm font-medium text-neutral-700">实例启动间隔（秒）</label>
              <input
                type="number"
                min={0}
                step={1}
                value={settings.instance_autostart_delay_secs}
                disabled={loading || saving || !settings.instance_autostart_enabled}
                onChange={(event) =>
                  updateSettings(
                    "instance_autostart_delay_secs",
                    Math.max(0, Number(event.target.value) || 0),
                  )
                }
                className="w-full rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-700 outline-none transition-colors focus:border-primary-400"
              />
              <p className="mt-2 text-xs text-neutral-500">
                用于多个实例顺序启动时的缓冲时间，避免同时拉起导致资源争抢。
              </p>
            </div>
          </div>
        </section>

        <section className="rounded-xl border border-neutral-200 bg-white p-6">
          <div className="mb-5 flex items-center gap-2 text-neutral-800">
            <Shield className="h-4 w-4 text-primary-500" />
            <h2 className="text-base font-semibold">网络代理</h2>
          </div>

          <div className="space-y-5">
            <ToggleRow
              title="启用网络代理"
              description="保存后会立即写入应用进程环境变量，并同步到后续创建、启动和进入终端的 VM 环境。"
              checked={settings.proxy_enabled}
              disabled={loading || saving}
              onChange={(checked) => updateSettings("proxy_enabled", checked)}
            />

            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <label className="mb-2 block text-sm font-medium text-neutral-700">代理地址</label>
                <input
                  type="text"
                  placeholder="例如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
                  value={settings.proxy_url ?? ""}
                  disabled={loading || saving || !settings.proxy_enabled}
                  onChange={(event) => updateSettings("proxy_url", event.target.value)}
                  className="w-full rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-700 outline-none transition-colors focus:border-primary-400"
                />
              </div>

              <div>
                <label className="mb-2 block text-sm font-medium text-neutral-700">不走代理</label>
                <input
                  type="text"
                  placeholder="例如 localhost,127.0.0.1,*.local"
                  value={settings.no_proxy ?? ""}
                  disabled={loading || saving || !settings.proxy_enabled}
                  onChange={(event) => updateSettings("no_proxy", event.target.value)}
                  className="w-full rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-700 outline-none transition-colors focus:border-primary-400"
                />
              </div>
            </div>

            <p className="text-xs leading-6 text-neutral-500">
              代理配置会写入目标 VM 的代理环境文件，OpenClaw 安装、实例启动、网页终端和手动打开的 VM shell 都会优先读取它。如果填写的是宿主机 127.0.0.1 或 localhost 代理，系统会按运行模式自动改写成 VM 可访问的宿主地址：macOS/Lima、Windows/WSL、Windows/QEMU 都会分别处理。
            </p>

            {proxyPreview && proxyPreview.original && (
              <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-4">
                <h3 className="text-sm font-medium text-neutral-800">代理地址预览</h3>
                <p className="mt-1 text-xs leading-6 text-neutral-500">
                  下面展示保存后在不同虚拟化环境内实际会使用的代理地址。
                </p>

                <div className="mt-4 grid gap-3 md:grid-cols-3">
                  {proxyPreview.lima_preview && (
                    <ProxyPreviewCard
                      title="Lima"
                      subtitle="macOS"
                      value={proxyPreview.lima_preview}
                    />
                  )}
                  {proxyPreview.wsl_preview && (
                    <ProxyPreviewCard
                      title="WSL"
                      subtitle="Windows"
                      value={proxyPreview.wsl_preview}
                    />
                  )}
                  {proxyPreview.qemu_preview && (
                    <ProxyPreviewCard
                      title="QEMU"
                      subtitle="Windows"
                      value={proxyPreview.qemu_preview}
                    />
                  )}
                </div>
              </div>
            )}
          </div>
        </section>

        <section className="rounded-xl border border-neutral-200 bg-white p-6">
          <div className="mb-5 flex items-center gap-2 text-neutral-800">
            <Trash2 className="h-4 w-4 text-primary-500" />
            <h2 className="text-base font-semibold">维护工具</h2>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-4">
              <h3 className="text-sm font-medium text-neutral-800">日志清除</h3>
              <p className="mt-2 text-sm leading-6 text-neutral-500">
                清理本地原生日志、PID 文件以及数据库中的监控指标记录。
              </p>
              <button onClick={handleClearLogs} className="btn-default mt-4" disabled={loading || clearingLogs}>
                {clearingLogs ? <Loader2 className="h-4 w-4 animate-spin" /> : <RotateCcw className="h-4 w-4" />}
                <span>立即清理</span>
              </button>
            </div>

            <div className="rounded-lg border border-dashed border-neutral-200 bg-neutral-50 p-4">
              <h3 className="text-sm font-medium text-neutral-800">检查更新</h3>
              <p className="mt-2 text-sm leading-6 text-neutral-500">
                该功能暂未接入，后续会补充版本检查和一键更新能力。
              </p>
              <button className="btn-default mt-4" disabled>
                <span>暂未开放</span>
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function ProxyPreviewCard({
  title,
  subtitle,
  value,
}: {
  title: string;
  subtitle: string;
  value: string;
}) {
  return (
    <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium text-neutral-800">{title}</span>
        <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] text-neutral-500">
          {subtitle}
        </span>
      </div>
      <p className="mt-2 break-all text-xs leading-6 text-primary-600">{value}</p>
    </div>
  );
}

function ToggleRow({
  title,
  description,
  checked,
  disabled,
  onChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-lg border border-neutral-100 bg-neutral-50 px-4 py-4">
      <div>
        <h3 className="text-sm font-medium text-neutral-800">{title}</h3>
        <p className="mt-1 text-sm leading-6 text-neutral-500">{description}</p>
      </div>

      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative mt-1 inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors ${
          checked ? "bg-primary-500" : "bg-neutral-300"
        } ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
      >
        <span
          className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
            checked ? "translate-x-5" : "translate-x-1"
          }`}
        />
      </button>
    </div>
  );
}