import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Loader2, Circle } from "lucide-react";
import { Terminal as XTerm } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import "xterm/css/xterm.css";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../lib/api";

export function WebShellPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string>("");
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(true);

  useEffect(() => {
    if (!containerRef.current || !id) return;

    const sessionId = `pty-${id}-${Date.now()}`;
    sessionIdRef.current = sessionId;

    const term = new XTerm({
      cursorBlink: true,
      fontFamily: "Menlo, Monaco, Consolas, 'Courier New', monospace",
      fontSize: 13,
      theme: {
        background: "#0b1220",
        foreground: "#d9e1ee",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    const unlisteners: UnlistenFn[] = [];

    // Listen for PTY output
    const setupListeners = async () => {
      const unlisten1 = await listen<string>(
        `pty-output-${sessionId}`,
        (event) => {
          term.write(event.payload);
        }
      );
      unlisteners.push(unlisten1);

      const unlisten2 = await listen(
        `pty-exit-${sessionId}`,
        () => {
          setConnected(false);
        }
      );
      unlisteners.push(unlisten2);
    };

    // Forward terminal input to PTY
    const disposable = term.onData((data: string) => {
      api.ptyWrite(sessionId, data).catch(() => {
        // Session may have ended
      });
    });

    // Handle resize
    const sendResize = () => {
      fit.fit();
      const dims = fit.proposeDimensions();
      if (dims) {
        api.ptyResize(sessionId, dims.rows, dims.cols).catch(() => {});
      }
    };

    const onResize = () => sendResize();
    window.addEventListener("resize", onResize);

    // Start the session
    const startSession = async () => {
      try {
        await setupListeners();
        const dims = fit.proposeDimensions();
        const rows = dims?.rows ?? 24;
        const cols = dims?.cols ?? 80;
        await api.ptySpawn(sessionId, id, rows, cols);
        setConnected(true);
        setConnecting(false);
      } catch (e) {
        term.writeln(`\r\n连接失败: ${String(e)}\r\n`);
        setConnecting(false);
      }
    };
    void startSession();

    return () => {
      window.removeEventListener("resize", onResize);
      disposable.dispose();
      for (const u of unlisteners) u();
      api.ptyClose(sessionId).catch(() => {});
      term.dispose();
      termRef.current = null;
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

      <div className="rounded-card border border-neutral-200 overflow-hidden bg-[#0b1220] flex-1 min-h-0">
        <div ref={containerRef} className="w-full h-full p-2" />
      </div>
    </div>
  );
}
