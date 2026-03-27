import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Loader2, Circle } from "lucide-react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../lib/api";

export function WebShellPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  // containerRef: the div xterm will render into
  const containerRef = useRef<HTMLDivElement | null>(null);
  // outerRef: the flex-1 wrapper — observed for real pixel dimensions
  const outerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string>("");
  const effectRunIdRef = useRef(0);
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(true);
  const [shellStage, setShellStage] = useState("等待初始化");
  const [inputCount, setInputCount] = useState(0);
  const [outputCount, setOutputCount] = useState(0);
  const [resizeCount, setResizeCount] = useState(0);
  const [lastError, setLastError] = useState<string | null>(null);
  const [lastSize, setLastSize] = useState("0 x 0");
  const [lastOutputPreview, setLastOutputPreview] = useState("");

  useEffect(() => {
    if (!containerRef.current || !outerRef.current || !id) return;

    effectRunIdRef.current += 1;
    const effectRunId = effectRunIdRef.current;
    setShellStage("准备终端");
    setInputCount(0);
    setOutputCount(0);
    setResizeCount(0);
    setLastError(null);
    setLastOutputPreview("");

    const sessionId = `pty-${id}-${Date.now()}`;
    sessionIdRef.current = sessionId;

    const term = new XTerm({
      cursorBlink: true,
      fontFamily: "Menlo, Monaco, Consolas, 'Courier New', monospace",
      fontSize: 13,
      scrollback: 5000,
      theme: {
        background: "#0b1220",
        foreground: "#d9e1ee",
        cursor: "#d9e1ee",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);

    termRef.current = term;
    fitRef.current = fit;

    const unlisteners: UnlistenFn[] = [];

    const setupListeners = async () => {
      const unlisten1 = await listen<string>(`pty-output-${sessionId}`, (event) => {
        setOutputCount((count) => count + event.payload.length);
        setLastOutputPreview((current) => {
          const next = `${current}${event.payload}`;
          return next.slice(-240);
        });
        term.write(event.payload);
      });
      unlisteners.push(unlisten1);

      const unlisten2 = await listen(`pty-exit-${sessionId}`, () => {
        setConnected(false);
        setShellStage("会话已退出");
      });
      unlisteners.push(unlisten2);
    };

    const disposable = term.onData((data: string) => {
      setInputCount((count) => count + data.length);
      api.ptyWrite(sessionId, data).catch((error) => {
        const text = String(error);
        setLastError(text);
        setShellStage("输入写入失败");
      });
    });

    const sendResize = () => {
      if (!fitRef.current || !termRef.current) return;
      try { fitRef.current.fit(); } catch (_) {}
      const dims = fitRef.current.proposeDimensions();
      if (dims) {
        setResizeCount((count) => count + 1);
        api.ptyResize(sessionId, dims.rows, dims.cols).catch((error) => {
          const text = String(error);
          setLastError(text);
          setShellStage("终端缩放失败");
        });
      }
    };

    window.addEventListener("resize", sendResize);

    const startSession = async () => {
      if (effectRunIdRef.current !== effectRunId) return;
      try {
        setShellStage("绑定输出事件");
        await setupListeners();
        if (effectRunIdRef.current !== effectRunId) return;
        const dims = fit.proposeDimensions();
        const rows = dims?.rows ?? 24;
        const cols = dims?.cols ?? 80;
        setShellStage(`启动 PTY ${rows}x${cols}`);
        await api.ptySpawn(sessionId, id, rows, cols);
        if (effectRunIdRef.current !== effectRunId) {
          void api.ptyClose(sessionId).catch(() => {});
          return;
        }
        setConnected(true);
        setConnecting(false);
        setShellStage("PTY 已连接");
        term.focus();
      } catch (e) {
        const text = String(e);
        setLastError(text);
        setShellStage("连接失败");
        term.writeln(`\r\n连接失败: ${text}\r\n`);
        setConnecting(false);
      }
    };

    // KEY FIX: In macOS WKWebView, calling term.open() or fit.fit() before the
    // container has real pixel dimensions causes:
    //   "TypeError: undefined is not an object (evaluating '_renderer.value.dimensions')"
    // requestAnimationFrame is not sufficient — flex layout may not have settled.
    // Use ResizeObserver on the OUTER wrapper div so we only open+fit when the
    // element has an actual non-zero size.
    let opened = false;
    const tryOpenTerminal = () => {
      if (opened || !outerRef.current || !containerRef.current) return false;
      const outerRect = outerRef.current.getBoundingClientRect();
      const containerRect = containerRef.current.getBoundingClientRect();
      setLastSize(`${Math.round(containerRect.width)} x ${Math.round(containerRect.height)}`);
      if (outerRect.width <= 0 || outerRect.height <= 0) return false;
      if (containerRect.width <= 0 || containerRect.height <= 0) return false;

      opened = true;
      ro.disconnect();
      if (effectRunIdRef.current !== effectRunId) return true;
      setShellStage("打开终端画布");
      term.open(containerRef.current);
      try { fit.fit(); } catch (_) {}
      term.focus();
      void startSession();
      return true;
    };

    const ro = new ResizeObserver(() => {
      tryOpenTerminal();
    });
    ro.observe(outerRef.current);
    ro.observe(containerRef.current);

    const rafId = window.requestAnimationFrame(() => {
      tryOpenTerminal();
    });

    let pollCount = 0;
    const pollTimer = window.setInterval(() => {
      pollCount += 1;
      if (tryOpenTerminal() || pollCount >= 20) {
        window.clearInterval(pollTimer);
      }
    }, 100);

    void Promise.resolve().then(() => {
      tryOpenTerminal();
    });

    return () => {
      window.cancelAnimationFrame(rafId);
      window.clearInterval(pollTimer);
      ro.disconnect();
      window.removeEventListener("resize", sendResize);
      disposable.dispose();
      for (const u of unlisteners) u();
      api.ptyClose(sessionId).catch(() => {});
      if (termRef.current) {
        termRef.current.dispose();
        termRef.current = null;
      }
      fitRef.current = null;
    };
  }, [id]);

  return (
    <div className="p-6 h-full flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <button className="btn-default" onClick={() => navigate(-1)}>
          <ArrowLeft className="w-3.5 h-3.5" />
          <span>返回</span>
        </button>
        <div className="text-caption text-neutral-500 flex items-center gap-1.5">
          {connecting && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
          {!connecting && (
            <Circle
              className={`w-2.5 h-2.5 ${connected ? "fill-green-500 text-green-500" : "fill-neutral-400 text-neutral-400"}`}
            />
          )}
          <span>
            {connecting
              ? "连接中..."
              : connected
                ? "已连接（PTY 交互模式）"
                : "已断开"}
          </span>
        </div>
      </div>

      <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 text-xs text-neutral-500">
        <div className="flex flex-wrap gap-x-4 gap-y-2">
          <span>阶段：{shellStage}</span>
          <span>尺寸：{lastSize}</span>
          <span>输入字节：{inputCount}</span>
          <span>输出字节：{outputCount}</span>
          <span>缩放次数：{resizeCount}</span>
          <span>会话：{sessionIdRef.current || "未创建"}</span>
        </div>
        {lastError && <div className="mt-2 text-red-600">最近错误：{lastError}</div>}
        {lastOutputPreview && (
          <div className="mt-2 whitespace-pre-wrap break-all text-neutral-600">
            最近输出：{lastOutputPreview}
          </div>
        )}
      </div>

      {/* outerRef: observed by ResizeObserver to detect real pixel dimensions */}
      <div
        ref={outerRef}
        className="rounded-card border border-neutral-200 overflow-hidden bg-[#0b1220] flex-1 min-h-0"
        onClick={() => termRef.current?.focus()}
      >
        {/* containerRef: xterm renders into this element; no padding — xterm manages its own */}
        <div ref={containerRef} className="w-full h-full" />
      </div>
    </div>
  );
}

