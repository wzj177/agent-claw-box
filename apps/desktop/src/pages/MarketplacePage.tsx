import { useEffect, useState } from "react";
import { Download, Search, Cpu, HardDrive, MemoryStick } from "lucide-react";
import { api, type TemplateInfo, type SystemInfo } from "../lib/api";
import { useNavigate } from "react-router-dom";

const INSTALL_METHOD_LABELS: Record<string, string> = {
  docker: "Docker 镜像",
  compose: "Docker Compose",
  script: "自定义构建",
  native: "原生安装",
};

export function MarketplacePage() {
  const navigate = useNavigate();
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
  const [deploying, setDeploying] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [deployError, setDeployError] = useState<string | null>(null);
  const [provisioning, setProvisioning] = useState(false);

  useEffect(() => {
    const load = async () => {
      try {
        const [tmpls, sys, prov] = await Promise.all([
          api.listTemplates(),
          api.getSystemInfo(),
          api.isProvisioning(),
        ]);
        setTemplates(tmpls);
        setSystemInfo(sys);
        setProvisioning(prov);
      } catch (e) {
        setLoadError(String(e));
      }
    };
    load();
  }, []);

  useEffect(() => {
    const refreshProvisioning = async () => {
      try {
        setProvisioning(await api.isProvisioning());
      } catch {
        // Ignore transient polling failures.
      }
    };

    const timer = setInterval(refreshProvisioning, 3_000);
    return () => clearInterval(timer);
  }, []);

  const filtered = templates.filter(
    (t) =>
      t.name.toLowerCase().includes(search.toLowerCase()) ||
      t.description.includes(search)
  );

  const handleDeploy = async (template: TemplateInfo) => {
    setDeploying(template.id);
    setDeployError(null);
    setProvisioning(true);
    try {
      await api.createAgent(template.name, template.id);
      navigate("/");
    } catch (e) {
      setDeployError(`${template.name} 部署失败: ${e}`);
      setProvisioning(false);
    } finally {
      setDeploying(null);
    }
  };

  return (
    <div className="p-6">
      {/* Header */}
      <div className="flex items-center justify-between mb-5">
        <h1 className="text-page-title text-neutral-800">应用市场</h1>
        {systemInfo && (
          <div className="flex items-center gap-4 text-caption text-neutral-400">
            <span className="flex items-center gap-1">
              <Cpu className="w-3.5 h-3.5" />
              {systemInfo.cpu_cores} 核
            </span>
            <span className="flex items-center gap-1">
              <MemoryStick className="w-3.5 h-3.5" />
              {Math.round(systemInfo.available_memory_mb / 1024)} GB 可用
            </span>
            <span className="flex items-center gap-1">
              <HardDrive className="w-3.5 h-3.5" />
              最多 {systemInfo.max_running} 个同时运行
            </span>
          </div>
        )}
      </div>

      {/* Search */}
      <div className="relative mb-5 max-w-md">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-neutral-400 pointer-events-none" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="搜索 Agent 模板…"
          className="w-full pl-9 pr-3 py-2 text-body bg-white border border-neutral-300 rounded-md
                     placeholder:text-neutral-400
                     focus:outline-none focus:border-primary-400 focus:ring-1 focus:ring-primary-100
                     transition-colors duration-150"
        />
      </div>

      {/* Error */}
      {loadError && (
        <div className="mb-4 px-4 py-3 rounded-md bg-red-50 text-red-600 text-caption">
          加载模板失败: {loadError}
        </div>
      )}
      {deployError && (
        <div className="mb-4 px-4 py-3 rounded-md bg-red-50 text-red-600 text-caption flex items-center justify-between">
          <span>{deployError}</span>
          <button onClick={() => setDeployError(null)} className="text-red-400 hover:text-red-600 ml-4">&times;</button>
        </div>
      )}

      {/* Grid */}
      <div className="grid gap-4 grid-cols-1 md:grid-cols-2 xl:grid-cols-3">
        {filtered.map((t) => (
          <div
            key={t.id}
            className="bg-white rounded-card border border-neutral-200 shadow-card hover:shadow-card-hover transition-shadow duration-200 flex flex-col"
          >
            <div className="px-4 pt-4 pb-3 flex-1">
              <div className="flex items-start justify-between mb-2">
                <h3 className="text-body font-medium text-neutral-800">{t.name}</h3>
                <span className="inline-flex items-center px-1.5 py-0.5 rounded text-caption text-neutral-400 bg-neutral-50 border border-neutral-100">
                  {INSTALL_METHOD_LABELS[t.install_method] ?? t.install_method}
                </span>
              </div>
              <p className="text-caption text-neutral-500 leading-relaxed mb-3">{t.description}</p>

              {/* Resource requirements */}
              <div className="flex items-center gap-3 text-caption text-neutral-400">
                <span>{t.resources.cpus} CPU</span>
                <span>{t.resources.memory_mb >= 1024 ? `${(t.resources.memory_mb / 1024).toFixed(1)} GB` : `${t.resources.memory_mb} MB`} 内存</span>
                <span>{t.resources.disk_gb} GB 磁盘</span>
              </div>

              {/* Config fields preview */}
              {t.config_schema.length > 0 && (
                <div className="mt-2 text-caption text-neutral-400">
                  需要配置: {t.config_schema.filter((f) => f.required).map((f) => f.label).join("、") || "无必填项"}
                </div>
              )}
            </div>
            <div className="px-4 pb-4 flex items-center gap-2">
              <span className="text-caption text-neutral-300">v{t.version}</span>
              <div className="flex-1" />
              <button
                onClick={() => handleDeploy(t)}
                disabled={deploying === t.id || provisioning}
                className="btn-primary"
              >
                <Download className="w-3.5 h-3.5" />
                {deploying === t.id ? "部署中…" : provisioning ? "等待中…" : "部署"}
              </button>
            </div>
          </div>
        ))}
      </div>

      {filtered.length === 0 && !loadError && (
        <div className="text-center py-16 text-caption text-neutral-400">
          没有找到匹配的模板
        </div>
      )}
    </div>
  );
}
