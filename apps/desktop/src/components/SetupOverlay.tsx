import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Box, Loader2, CheckCircle2, XCircle, Terminal, Copy, Check } from "lucide-react";

interface SetupEvent {
  stage: string;
  message: string;
  done: boolean;
}

/**
 * Full-screen overlay shown during VM environment initialization.
 * Listens to "setup-progress" Tauri events emitted from the Rust backend.
 * Automatically hides once setup is complete or on error.
 */
const BREW_INSTALL_CMD =
  '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"';

export function SetupOverlay({ onReady }: { onReady: () => void }) {
  const [message, setMessage] = useState("正在初始化运行环境...");
  const [stage, setStage] = useState<"loading" | "ready" | "error" | "needs-brew">("loading");
  const [errorDetail, setErrorDetail] = useState("");
  const [steps, setSteps] = useState<string[]>([]);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;

    listen<SetupEvent>("setup-progress", (event) => {
      const { stage: s, message: msg, done } = event.payload;
      setMessage(msg);
      setSteps((prev) => {
        // Avoid duplicate messages
        if (prev.length > 0 && prev[prev.length - 1] === msg) return prev;
        return [...prev, msg];
      });

      if (done) {
        if (s === "error") {
          setStage("error");
          setErrorDetail(msg);
        } else if (s === "needs-brew") {
          setStage("needs-brew");
        } else {
          setStage("ready");
          setTimeout(() => onReady(), 800);
        }
      }
    }).then((fn) => {
      unlisten = fn;
    });

    // Timeout: if no event received and no steps logged, assume backend is ready
    // (fast startup when VM is already running).
    // If steps are already showing, setup is in progress — do NOT auto-dismiss.
    const timeout = setTimeout(() => {
      setStage((current) => {
        if (current === "loading") {
          setSteps((currentSteps) => {
            if (currentSteps.length === 0) {
              onReady();
              setStage("ready");
            }
            return currentSteps;
          });
        }
        return current;
      });
    }, 5000);

    return () => {
      unlisten?.();
      clearTimeout(timeout);
    };
  }, [onReady]);

  if (stage === "ready") return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-white">
      <div className="flex flex-col items-center max-w-md mx-auto px-6">
        {/* Logo */}
        <div className="w-16 h-16 rounded-2xl bg-primary-500 flex items-center justify-center mb-6 shadow-lg">
          <Box className="w-8 h-8 text-white" />
        </div>

        <h1 className="text-xl font-semibold text-neutral-800 mb-2">AgentBox</h1>

        {stage === "needs-brew" ? (
          <>
            <div className="flex items-center gap-2 text-amber-500 mb-3">
              <Terminal className="w-5 h-5" />
              <span className="text-body font-semibold">需要安装 Homebrew</span>
            </div>
            <p className="text-caption text-neutral-500 text-center mb-4">
              AgentBox 需要 Homebrew 来安装 Lima 虚拟环境。<br />
              请在终端运行以下命令，安装完成后重启应用。
            </p>
            <div className="w-full bg-neutral-900 rounded-lg p-3 mb-3 relative">
              <code className="text-xs text-green-400 break-all leading-relaxed">
                {BREW_INSTALL_CMD}
              </code>
              <button
                onClick={() => {
                  navigator.clipboard.writeText(BREW_INSTALL_CMD);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                }}
                className="absolute top-2 right-2 p-1 rounded text-neutral-400 hover:text-white transition-colors"
                title="复制命令"
              >
                {copied ? <Check className="w-4 h-4 text-green-400" /> : <Copy className="w-4 h-4" />}
              </button>
            </div>
            <p className="text-caption text-neutral-400 mb-5">
              安装后根据终端提示将 Homebrew 加入 PATH，然后：
            </p>
            <button
              onClick={() => window.location.reload()}
              className="btn-primary"
            >
              已安装，重新检测
            </button>
          </>
        ) : stage === "error" ? (
          <>
            <div className="flex items-center gap-2 text-red-500 mb-4">
              <XCircle className="w-5 h-5" />
              <span className="text-body font-medium">环境初始化失败</span>
            </div>
            <p className="text-caption text-neutral-500 text-center mb-6">
              {errorDetail}
            </p>
            <button
              onClick={() => window.location.reload()}
              className="btn-primary"
            >
              重试
            </button>
          </>
        ) : (
          <>
            {/* Spinner */}
            <Loader2 className="w-6 h-6 text-primary-500 animate-spin mb-4" />
            <p className="text-body text-neutral-600 mb-6">{message}</p>

            {/* Step log */}
            {steps.length > 0 && (
              <div className="w-full max-h-40 overflow-y-auto bg-neutral-50 rounded-lg border border-neutral-200 p-3">
                {steps.map((step, i) => (
                  <div key={i} className="flex items-start gap-2 py-0.5">
                    {i < steps.length - 1 ? (
                      <CheckCircle2 className="w-3.5 h-3.5 text-green-500 mt-0.5 shrink-0" />
                    ) : (
                      <Loader2 className="w-3.5 h-3.5 text-primary-500 animate-spin mt-0.5 shrink-0" />
                    )}
                    <span className="text-caption text-neutral-500">{step}</span>
                  </div>
                ))}
              </div>
            )}
          </>
        )}

        <p className="text-caption text-neutral-400 mt-8">
          首次启动需要初始化运行环境，请耐心等待
        </p>
      </div>
    </div>
  );
}
