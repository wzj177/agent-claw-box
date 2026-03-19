import { useEffect, useState } from "react";
import {
  Play,
  Square,
  Trash2,
  Terminal,
  Monitor,
  ExternalLink,
  RefreshCw,
  Power,
  PowerOff,
  Settings,
  ArrowUpCircle,
  Download,
  ScrollText,
  Loader2,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { api, type AgentInfo, type AgentStatus, type HealthReport, type TemplateInfo } from "../lib/api";

export function AgentsPage() {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [health, setHealth] = useState<Map<string, HealthReport>>(new Map());
  const [loading, setLoading] = useState(true);

  const refresh = async () => {
    setLoading(true);
    try {
      const [list, reports] = await Promise.all([
        api.listAgents(),
        api.getHealthReports(),
      ]);
      setAgents(list);
      const m = new Map<string, HealthReport>();
      for (const r of reports) m.set(r.agent_id, r);
      setHealth(m);
    } finally {
      setLoading(false);
    }
  };

  const hasPendingTransition = agents.some((a) => a.status === "CREATING" || a.status === "STARTING");

  useEffect(() => {
    refresh();
  }, []);

  // Poll faster when any agent is being created
  useEffect(() => {
    const timer = setInterval(refresh, hasPendingTransition ? 5_000 : 15_000);
    return () => clearInterval(timer);
  }, [hasPendingTransition]);

  return (
    <div className="p-6">
      {/* Page header */}
      <div className="flex items-center justify-between mb-5">
        <h1 className="text-page-title text-neutral-800">我的实例</h1>
        <button onClick={refresh} className="btn-default" title="刷新">
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
          <span>刷新</span>
        </button>
      </div>

      {/* Empty state */}
      {agents.length === 0 && !loading && (
        <div className="flex flex-col items-center justify-center py-24">
          <div className="w-16 h-16 rounded-full bg-neutral-100 flex items-center justify-center mb-4">
            <Power className="w-7 h-7 text-neutral-300" />
          </div>
          <p className="text-body text-neutral-500 mb-1">暂无实例</p>
          <p className="text-caption text-neutral-400">前往应用市场部署你的第一个 Agent</p>
        </div>
      )}

      {/* Agent list */}
      <div className="grid gap-4 grid-cols-1 lg:grid-cols-2 xl:grid-cols-3">
        {agents.map((agent) => (
          <AgentCard
            key={agent.id}
            agent={agent}
            health={health.get(agent.id)}
            onRefresh={refresh}
          />
        ))}
      </div>
      {/* System hint */}
      <SystemHint />
    </div>
  );
}

function AgentCard({
  agent,
  health: report,
  onRefresh,
}: {
  agent: AgentInfo;
  health?: HealthReport;
  onRefresh: () => void;
}) {
  const navigate = useNavigate();
  const isRunning = agent.status === "RUNNING";
  const isCreating = agent.status === "CREATING";
  const isStarting = agent.status === "STARTING";
  const isTransitioning = isCreating || isStarting;
  const isFailed = agent.status === "CREATE_FAILED" || agent.status === "START_FAILED";
  const healthy = report?.healthy ?? false;

  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [needsConfig, setNeedsConfig] = useState(false);
  const [showConfigOverlay, setShowConfigOverlay] = useState(false);

  // Check if required config fields are filled
  useEffect(() => {
    let cancelled = false;
    const check = async () => {
      try {
        const [templates, configs] = await Promise.all([
          api.listTemplates(),
          api.getAgentConfig(agent.id),
        ]);
        const tmpl = templates.find((t: TemplateInfo) => t.id === agent.template);
        if (!tmpl || cancelled) return;
        const requiredFields = tmpl.config_schema.filter((f) => f.required);
        if (requiredFields.length === 0) { setNeedsConfig(false); return; }
        const configMap = new Map(configs.map((c) => [c.config_key, c.config_value]));
        const provider = configMap.get("llm_provider") ?? "";
        const missing = requiredFields.some((f) => {
          // ollama doesn't require api_key
          if (f.key === "api_key" && provider === "ollama") return false;
          return !configMap.get(f.key) || configMap.get(f.key) === "";
        });
        if (!cancelled) setNeedsConfig(missing);
      } catch { /* ignore */ }
    };
    check();
    return () => { cancelled = true; };
  }, [agent.id, agent.template]);

  // Elapsed time counter for CREATING state
  useEffect(() => {
    if (!isCreating) { setElapsed(0); return; }
    const start = new Date(agent.created_at).getTime();
    const tick = () => setElapsed(Math.floor((Date.now() - start) / 1000));
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  }, [isCreating, agent.created_at]);

  // 自动取消删除确认 (3s)
  useEffect(() => {
    if (!confirmingDelete) return;
    const t = setTimeout(() => setConfirmingDelete(false), 3000);
    return () => clearTimeout(t);
  }, [confirmingDelete]);

  // 自动隐藏提示 (4s)
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  /** 通用操作包装：加 loading + 错误提示 */
  const run = (fn: () => Promise<void>) => async () => {
    if (busy) return;
    setBusy(true);
    setToast(null);
    try {
      await fn();
      onRefresh();
    } catch (e) {
      setToast({ msg: String(e), ok: false });
    } finally {
      setBusy(false);
    }
  };

  const handleStart = run(() => api.startAgent(agent.id));
  const handleStop = run(() => api.stopAgent(agent.id));
  const handleToggleAutoStart = run(() => api.setAutoStart(agent.id, !agent.auto_start));
  const handleUpgrade = run(() => api.upgradeAgent(agent.id).then(() => undefined));

  const handleExport = async () => {
    if (busy) return;
    setBusy(true);
    setToast(null);
    try {
      const path = await api.exportAgentData(agent.id);
      setToast({ msg: `备份已导出至: ${path}`, ok: true });
    } catch (e) {
      setToast({ msg: `导出失败: ${e}`, ok: false });
    } finally {
      setBusy(false);
    }
  };

  const handleDeleteClick = async () => {
    if (busy) return;
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    // 第二次点击：执行删除
    setConfirmingDelete(false);
    setBusy(true);
    setToast(null);
    try {
      await api.deleteAgent(agent.id);
      onRefresh();
    } catch (e) {
      setToast({ msg: `删除失败: ${e}`, ok: false });
      setBusy(false);
    }
  };

  const handleShellLocal = async () => {
    try {
      await api.openAgentShell(agent.id);
      if (agent.install_method === "native") {
        setToast({ msg: "已打开本地终端并连接到 VM", ok: true });
      }
    } catch (e) {
      setToast({ msg: String(e), ok: false });
    }
  };
  const handleShellWeb = () => navigate(`/shell/${agent.id}`);
  const handleBrowser = async () => {
    if (needsConfig) {
      setShowConfigOverlay(true);
      return;
    }
    try { await api.openAgentBrowser(agent.id); } catch (e) { setToast({ msg: String(e), ok: false }); }
  };

  return (
    <div className="relative bg-white rounded-card border border-neutral-200 shadow-card hover:shadow-card-hover transition-shadow duration-200">
      {/* Card header */}
      <div className="px-4 py-3 flex items-center justify-between border-b border-neutral-100">
        <div className="flex items-center gap-3 min-w-0">
          <div className={`w-2 h-2 rounded-full shrink-0 ${
            isRunning
              ? (healthy || !report ? "bg-green-500" : "bg-yellow-500")
              : isTransitioning
                ? "bg-blue-500"
                : isFailed
                  ? "bg-red-500"
                  : "bg-neutral-300"
          }`} />
          <div className="min-w-0">
            <h3
              className="text-body font-medium text-neutral-800 truncate cursor-pointer hover:text-primary-500 transition-colors"
              onClick={() => navigate(`/agent/${agent.id}`)}
              title="查看日志与监控"
            >
              {agent.name}
            </h3>
            <p className="text-caption text-neutral-400">
              {agent.template} · 实例 #{agent.instance_no} · v{agent.version}
            </p>
          </div>
        </div>
        <StatusTag status={agent.status} />
      </div>

      {/* Card body */}
      <div className="px-4 py-3 space-y-2">
        <InfoRow label="端口" value={String(agent.port)} />
        <InfoRow label="开机自启" value={agent.auto_start ? "已开启" : "未开启"} />
        {agent.install_method === "native" && (
          <InfoRow label="SSH" value={`ssh ${agent.vm_name}`} valueClass="text-xs font-mono select-all" />
        )}
        {isCreating && (
          <InfoRow
            label="已用时"
            value={elapsed < 60 ? `${elapsed} 秒` : `${Math.floor(elapsed / 60)} 分 ${elapsed % 60} 秒`}
            valueClass="text-blue-500"
          />
        )}
        {report && !isTransitioning && (
          <InfoRow
            label="健康状态"
            value={report.detail}
            valueClass={report.healthy ? "text-green-600" : "text-red-500"}
          />
        )}
      </div>

      {/* Card actions */}
      <div className="px-3 py-2.5 flex items-center gap-0.5 border-t border-neutral-100">
        {isTransitioning ? (
          <div className="flex items-center gap-1.5 pl-1 text-blue-500">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span className="text-caption">{isCreating ? "创建中..." : "启动中..."}</span>
          </div>
        ) : isRunning ? (
          <button onClick={handleStop} disabled={busy} className="btn-text" title="停止">
            <Square className="w-3.5 h-3.5" />
            <span className="text-caption">停止</span>
          </button>
        ) : (
          <button onClick={handleStart} disabled={busy} className="btn-text" title="启动">
            <Play className="w-3.5 h-3.5" />
            <span className="text-caption">启动</span>
          </button>
        )}

        <div className="w-px h-4 bg-neutral-200 mx-1" />

        <button onClick={handleShellLocal} disabled={busy || isTransitioning} className="btn-text" title="打开本地终端">
          <Terminal className="w-3.5 h-3.5" />
        </button>
        <button onClick={handleShellWeb} disabled={busy || isTransitioning} className="btn-text" title="Web Shell">
          <Monitor className="w-3.5 h-3.5" />
        </button>
        <button onClick={() => navigate(`/agent/${agent.id}`)} className="btn-text" title="日志与监控">
          <ScrollText className="w-3.5 h-3.5" />
        </button>
        <button onClick={handleBrowser} disabled={busy || isTransitioning} className="btn-text" title="在浏览器中打开">
          <ExternalLink className="w-3.5 h-3.5" />
        </button>
        <button onClick={() => navigate(`/config/${agent.id}`)} className="btn-text" title="配置">
          <Settings className="w-3.5 h-3.5" />
        </button>
        <button onClick={handleExport} disabled={busy || isTransitioning} className="btn-text" title="导出备份">
          <Download className="w-3.5 h-3.5" />
        </button>
        <button onClick={handleUpgrade} disabled={busy || isTransitioning} className="btn-text" title="升级">
          <ArrowUpCircle className="w-3.5 h-3.5" />
        </button>
        <button onClick={handleToggleAutoStart} disabled={busy || isTransitioning} className="btn-text" title="切换开机自启">
          {agent.auto_start
            ? <Power className="w-3.5 h-3.5 text-primary-500" />
            : <PowerOff className="w-3.5 h-3.5" />
          }
        </button>

        <div className="flex-1" />

        {confirmingDelete ? (
          <button
            onClick={handleDeleteClick}
            className="btn-danger-text animate-pulse"
            title={isCreating ? "再次点击确认取消创建并删除，删除后不可恢复" : "再次点击确认彻底删除，删除后不可恢复"}
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span className="text-caption">{isCreating ? "确认取消" : "确认删除"}</span>
          </button>
        ) : (
          <button
            onClick={handleDeleteClick}
            disabled={busy}
            className="btn-danger-text"
            title={isCreating ? "取消创建并删除，删除后不可恢复" : "删除实例，删除后不可恢复"}
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span className="text-caption">{isCreating ? "取消创建" : "删除"}</span>
          </button>
        )}
      </div>

      <div className="px-4 py-2 text-caption border-t border-red-100 bg-red-50 text-red-600">
        {isCreating ? "取消创建会删除已创建的半成品虚拟机，且不可恢复。" : "删除实例会彻底删除对应虚拟机，删除后不可恢复。"}
      </div>

      {/* Config overlay */}
      {showConfigOverlay && needsConfig && (
        <div className="absolute inset-0 z-10 bg-white/80 backdrop-blur-sm rounded-card flex flex-col items-center justify-center gap-3 px-6">
          <Settings className="w-8 h-8 text-primary-500" />
          <p className="text-body font-medium text-neutral-700 text-center">需要先完成配置</p>
          <p className="text-caption text-neutral-400 text-center">请配置 API Key 等必要参数后，才能访问 Web 界面</p>
          <div className="flex gap-2 mt-1">
            <button
              onClick={() => navigate(`/config/${agent.id}`)}
              className="btn-primary"
            >
              前往配置
            </button>
            <button
              onClick={() => setShowConfigOverlay(false)}
              className="btn-default"
            >
              稍后再说
            </button>
          </div>
        </div>
      )}

      {/* Config hint banner */}
      {needsConfig && !showConfigOverlay && (
        <div
          className="px-4 py-2 text-caption border-t border-amber-100 bg-amber-50 text-amber-700 flex items-center justify-between cursor-pointer"
          onClick={() => navigate(`/config/${agent.id}`)}
        >
          <span>尚未配置必要参数（如 API Key），点击前往配置</span>
          <Settings className="w-3.5 h-3.5" />
        </div>
      )}

      {/* Toast 提示 */}
      {toast && (
        <div className={`px-4 py-2 text-caption border-t ${toast.ok ? "bg-green-50 text-green-700 border-green-100" : "bg-red-50 text-red-600 border-red-100"}`}>
          {toast.msg}
        </div>
      )}
    </div>
  );
}

function InfoRow({
  label,
  value,
  valueClass = "text-neutral-700",
}: {
  label: string;
  value: string;
  valueClass?: string;
}) {
  return (
    <div className="flex items-center justify-between text-caption">
      <span className="text-neutral-400">{label}</span>
      <span className={valueClass}>{value}</span>
    </div>
  );
}

function StatusTag({ status }: { status: AgentStatus }) {
  const config: Record<AgentStatus, { bg: string; text: string; label: string }> = {
    CREATING: { bg: "bg-blue-50", text: "text-blue-600", label: "创建中" },
    CREATE_FAILED: { bg: "bg-red-50", text: "text-red-600", label: "创建失败" },
    PENDING: { bg: "bg-neutral-100", text: "text-neutral-500", label: "待启动" },
    STARTING: { bg: "bg-sky-50", text: "text-sky-600", label: "启动中" },
    RUNNING: { bg: "bg-green-50", text: "text-green-600", label: "已启动" },
    START_FAILED: { bg: "bg-red-50", text: "text-red-600", label: "启动异常" },
  };

  const c = config[status];

  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-caption font-medium ${c.bg} ${c.text}`}>
      {c.label}
    </span>
  );
}

function SystemHint() {
  const [info, setInfo] = useState<{
    cpu_cores: number;
    total_memory_mb: number;
    max_running: number;
  } | null>(null);

  useEffect(() => {
    api.getSystemInfo().then(setInfo).catch(() => {});
  }, []);

  if (!info) return null;

  return (
    <div className="mt-6 text-caption text-neutral-400 flex items-center gap-4">
      <span>系统: {info.cpu_cores} 核 · {Math.round(info.total_memory_mb / 1024)} GB 内存</span>
      <span>最多同时运行 {info.max_running} 个实例</span>
    </div>
  );
}
