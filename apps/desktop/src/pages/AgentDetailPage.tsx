import { useEffect, useRef, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  ArrowLeft,
  RefreshCw,
  Copy,
  CheckCircle2,
  Activity,
  ScrollText,
  Cpu,
  MemoryStick,
  ArrowDownUp,
} from "lucide-react";
import { api, type AgentInfo, type AgentMetrics } from "../lib/api";

type Tab = "logs" | "metrics";

export function AgentDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [tab, setTab] = useState<Tab>("logs");

  useEffect(() => {
    if (!id) return;
    api.listAgents().then((list) => {
      const found = list.find((a) => a.id === id);
      if (found) setAgent(found);
    });
  }, [id]);

  if (!id) return null;

  return (
    <div className="p-6 h-full flex flex-col">
      {/* Header */}
      <div className="flex items-center gap-3 mb-4">
        <button onClick={() => navigate(-1)} className="btn-text" title="返回">
          <ArrowLeft className="w-4 h-4" />
        </button>
        <div className="min-w-0">
          <h1 className="text-page-title text-neutral-800 truncate">
            {agent?.name ?? "加载中..."}
          </h1>
          {agent && (
            <p className="text-caption text-neutral-400">
              {agent.template} · 实例 #{agent.instance_no} · 端口 {agent.port}
            </p>
          )}
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b border-neutral-200 mb-4">
        <TabButton
          active={tab === "logs"}
          onClick={() => setTab("logs")}
          icon={<ScrollText className="w-3.5 h-3.5" />}
          label="日志"
        />
        <TabButton
          active={tab === "metrics"}
          onClick={() => setTab("metrics")}
          icon={<Activity className="w-3.5 h-3.5" />}
          label="监控"
        />
      </div>

      {/* Tab content */}
      <div className="flex-1 min-h-0">
        {tab === "logs" && <LogsPanel agentId={id} />}
        {tab === "metrics" && <MetricsPanel agentId={id} agent={agent} />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab button
// ---------------------------------------------------------------------------

function TabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 px-3 py-2 text-sm font-medium transition-colors
        ${active ? "text-primary-600 border-b-2 border-primary-500 -mb-px" : "text-neutral-500 hover:text-neutral-700"}`}
    >
      {icon}
      {label}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Logs panel
// ---------------------------------------------------------------------------

function LogsPanel({ agentId }: { agentId: string }) {
  const [logs, setLogs] = useState("");
  const [tail, setTail] = useState(200);
  const [loading, setLoading] = useState(false);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [copied, setCopied] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const fetchLogs = async () => {
    setLoading(true);
    try {
      const text = await api.getAgentLogs(agentId, tail);
      setLogs(text);
      // Scroll to bottom
      requestAnimationFrame(() => {
        if (logRef.current) {
          logRef.current.scrollTop = logRef.current.scrollHeight;
        }
      });
    } catch (e) {
      setLogs(`获取日志失败: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs();
  }, [agentId, tail]);

  useEffect(() => {
    if (!autoRefresh) return;
    const timer = setInterval(fetchLogs, 5_000);
    return () => clearInterval(timer);
  }, [agentId, tail, autoRefresh]);

  const handleCopy = () => {
    navigator.clipboard.writeText(logs);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 mb-2">
        <select
          value={tail}
          onChange={(e) => setTail(Number(e.target.value))}
          className="text-caption border border-neutral-200 rounded px-2 py-1 bg-white"
        >
          <option value={50}>最近 50 行</option>
          <option value={200}>最近 200 行</option>
          <option value={500}>最近 500 行</option>
          <option value={1000}>最近 1000 行</option>
        </select>

        <label className="flex items-center gap-1 text-caption text-neutral-500 cursor-pointer">
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => setAutoRefresh(e.target.checked)}
            className="rounded"
          />
          自动刷新
        </label>

        <div className="flex-1" />

        <button onClick={handleCopy} className="btn-text" title="复制日志">
          {copied ? (
            <CheckCircle2 className="w-3.5 h-3.5 text-green-500" />
          ) : (
            <Copy className="w-3.5 h-3.5" />
          )}
          <span className="text-caption">{copied ? "已复制" : "复制"}</span>
        </button>

        <button onClick={fetchLogs} className="btn-text" title="刷新">
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {/* Log content */}
      <pre
        ref={logRef}
        className="flex-1 min-h-0 overflow-auto bg-neutral-900 text-neutral-100 rounded-lg
                   p-4 text-xs font-mono leading-relaxed whitespace-pre-wrap break-all
                   selection:bg-primary-500/30"
      >
        {logs || (loading ? "加载中..." : "暂无日志")}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Metrics panel
// ---------------------------------------------------------------------------

function MetricsPanel({
  agentId,
  agent,
}: {
  agentId: string;
  agent: AgentInfo | null;
}) {
  const [metrics, setMetrics] = useState<AgentMetrics[]>([]);
  const [loading, setLoading] = useState(false);

  const fetchMetrics = async () => {
    setLoading(true);
    try {
      const data = await api.getAgentMetrics(agentId, 60);
      // API returns DESC order, reverse for chronological display
      setMetrics(data.reverse());
    } catch {
      setMetrics([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchMetrics();
    const timer = setInterval(fetchMetrics, 30_000);
    return () => clearInterval(timer);
  }, [agentId]);

  const latest = metrics.length > 0 ? metrics[metrics.length - 1] : null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between gap-3">
        <div>
          <p className="text-sm font-medium text-neutral-700">实例监控</p>
          <p className="text-caption text-neutral-400">
            每 30 秒刷新一次最新指标
          </p>
        </div>
        <button onClick={fetchMetrics} className="btn-text" title="刷新监控">
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
          <span className="text-caption">刷新</span>
        </button>
      </div>

      {agent?.install_method === "native" && (
        <div className="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800">
          当前原生实例已接入 CPU、内存、健康状态和网络累计流量采集。网络指标为实例启动后的累计值。
        </div>
      )}

      {/* Summary cards */}
      <div className="grid grid-cols-2 lg:grid-cols-5 gap-4">
        <StatCard
          icon={<Cpu className="w-5 h-5 text-blue-500" />}
          label="CPU 使用率"
          value={latest ? `${latest.cpu_percent.toFixed(1)}%` : "--"}
        />
        <StatCard
          icon={<MemoryStick className="w-5 h-5 text-purple-500" />}
          label="内存使用"
          value={latest ? formatMB(latest.memory_mb) : "--"}
        />
        <StatCard
          icon={<ArrowDownUp className="w-5 h-5 text-green-500" />}
          label="网络接收"
          value={latest ? formatKB(latest.net_rx_kb) : "--"}
        />
        <StatCard
          icon={<ArrowDownUp className="w-5 h-5 text-orange-500" />}
          label="网络发送"
          value={latest ? formatKB(latest.net_tx_kb) : "--"}
        />
        <StatCard
          icon={<Activity className={`w-5 h-5 ${latest?.healthy ? "text-emerald-500" : "text-rose-500"}`} />}
          label="健康状态"
          value={latest ? (latest.healthy ? "正常" : "异常") : "--"}
        />
      </div>

      {/* Charts */}
      {metrics.length > 0 ? (
        <div className="space-y-4">
          <MiniChart
            title="CPU 使用率 (%)"
            data={metrics.map((m) => m.cpu_percent)}
            color="#3b82f6"
            maxY={100}
          />
          <MiniChart
            title="内存使用 (MB)"
            data={metrics.map((m) => m.memory_mb)}
            color="#8b5cf6"
          />
          <MiniChart
            title="网络 I/O (KB)"
            data={metrics.map((m) => m.net_rx_kb + m.net_tx_kb)}
            color="#22c55e"
          />
        </div>
      ) : (
        <div className="flex items-center justify-center py-16 text-neutral-400">
          {loading ? (
            <RefreshCw className="w-5 h-5 animate-spin" />
          ) : (
            <p>暂无监控数据，等待采集中...</p>
          )}
        </div>
      )}

      {/* Recent records table */}
      {metrics.length > 0 && (
        <div>
          <h3 className="text-sm font-medium text-neutral-700 mb-2">最近记录</h3>
          <div className="overflow-auto max-h-64 border rounded-lg">
            <table className="w-full text-xs">
              <thead className="bg-neutral-50 sticky top-0">
                <tr>
                  <th className="px-3 py-2 text-left text-neutral-500 font-medium">时间</th>
                  <th className="px-3 py-2 text-right text-neutral-500 font-medium">CPU %</th>
                  <th className="px-3 py-2 text-right text-neutral-500 font-medium">内存</th>
                  <th className="px-3 py-2 text-right text-neutral-500 font-medium">网络 RX</th>
                  <th className="px-3 py-2 text-right text-neutral-500 font-medium">网络 TX</th>
                </tr>
              </thead>
              <tbody>
                {[...metrics].reverse().slice(0, 20).map((m, i) => (
                  <tr key={i} className="border-t border-neutral-100 hover:bg-neutral-50">
                    <td className="px-3 py-1.5 text-neutral-600">{formatTime(m.recorded_at)}</td>
                    <td className="px-3 py-1.5 text-right font-mono">{m.cpu_percent.toFixed(1)}</td>
                    <td className="px-3 py-1.5 text-right font-mono">{formatMB(m.memory_mb)}</td>
                    <td className="px-3 py-1.5 text-right font-mono">{formatKB(m.net_rx_kb)}</td>
                    <td className="px-3 py-1.5 text-right font-mono">{formatKB(m.net_tx_kb)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function StatCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="bg-white border border-neutral-200 rounded-card p-4 flex items-center gap-3 shadow-card">
      <div className="w-10 h-10 rounded-lg bg-neutral-50 flex items-center justify-center shrink-0">
        {icon}
      </div>
      <div>
        <p className="text-caption text-neutral-400">{label}</p>
        <p className="text-lg font-semibold text-neutral-800">{value}</p>
      </div>
    </div>
  );
}

/** Simple SVG area chart — no dependencies needed. */
function MiniChart({
  title,
  data,
  color,
  maxY,
}: {
  title: string;
  data: number[];
  color: string;
  maxY?: number;
}) {
  const width = 600;
  const height = 100;
  const pad = 2;

  const max = maxY ?? Math.max(...data, 1);
  const step = (width - pad * 2) / Math.max(data.length - 1, 1);

  const points = data.map((v, i) => {
    const x = pad + i * step;
    const y = height - pad - ((v / max) * (height - pad * 2));
    return `${x},${y}`;
  });

  const linePath = `M${points.join(" L")}`;
  const areaPath = `${linePath} L${pad + (data.length - 1) * step},${height - pad} L${pad},${height - pad} Z`;

  return (
    <div className="bg-white border border-neutral-200 rounded-card p-4 shadow-card">
      <h4 className="text-caption text-neutral-500 mb-2">{title}</h4>
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-24" preserveAspectRatio="none">
        <defs>
          <linearGradient id={`grad-${color.replace('#', '')}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity="0.3" />
            <stop offset="100%" stopColor={color} stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {data.length > 1 && (
          <>
            <path d={areaPath} fill={`url(#grad-${color.replace('#', '')})`} />
            <path d={linePath} fill="none" stroke={color} strokeWidth="2" vectorEffect="non-scaling-stroke" />
          </>
        )}
        {/* Latest value dot */}
        {data.length > 0 && (
          <circle
            cx={pad + (data.length - 1) * step}
            cy={height - pad - ((data[data.length - 1] / max) * (height - pad * 2))}
            r="3"
            fill={color}
          />
        )}
      </svg>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

function formatMB(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb.toFixed(1)} MB`;
}

function formatKB(kb: number): string {
  if (kb >= 1024 * 1024) return `${(kb / 1024 / 1024).toFixed(1)} GB`;
  if (kb >= 1024) return `${(kb / 1024).toFixed(1)} MB`;
  return `${kb.toFixed(1)} KB`;
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  } catch {
    return iso;
  }
}
