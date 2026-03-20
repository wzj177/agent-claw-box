import { ExternalLink, HelpCircle, BookOpen, MessageSquare, ChevronRight } from "lucide-react";
import { open } from "@tauri-apps/plugin-shell";
import { useNavigate } from "react-router-dom";

type CardType = "route" | "external";

interface Card {
  key: string;
  icon: React.ReactNode;
  title: string;
  desc: string;
  bg: string;
  action: string;
  type: CardType;
  target: string;
}

const CARDS: Card[] = [
  {
    key: "faq",
    icon: <HelpCircle className="w-7 h-7 text-primary-500" />,
    title: "常见问题",
    desc: "安装、部署、配置、网络等高频问题解答，快速定位并解决使用过程中的疑问。",
    bg: "bg-blue-50",
    action: "查看常见问题",
    type: "route",
    target: "/docs/faq",
  },
  {
    key: "guide",
    icon: <BookOpen className="w-7 h-7 text-emerald-500" />,
    title: "使用教程",
    desc: "从安装到部署第一个 Agent，图文并茂的完整使用指南，适合初次上手的用户。",
    bg: "bg-emerald-50",
    action: "查看使用教程",
    type: "route",
    target: "/docs/guide",
  },
  {
    key: "feedback",
    icon: <MessageSquare className="w-7 h-7 text-orange-500" />,
    title: "问题反馈",
    desc: "遇到 Bug 或有改进建议？欢迎告诉我，每条反馈都会认真查看。",
    bg: "bg-orange-50",
    action: "前往反馈",
    type: "external",
    target: "https://wanzij.cn/post-81.html",
  },
];

export function HelpCenterPage() {
  const navigate = useNavigate();

  async function handleOpen(card: Card) {
    if (card.type === "route") {
      navigate(card.target);
    } else {
      try {
        await open(card.target);
      } catch {
        window.open(card.target, "_blank", "noopener,noreferrer");
      }
    }
  }

  return (
    <div className="min-h-full bg-neutral-50 px-6 py-8">
      <div className="max-w-3xl mx-auto">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-xl font-semibold text-neutral-800">帮助中心</h1>
          <p className="mt-1 text-sm text-neutral-500">
            查阅文档、了解功能、反馈问题，一站解决使用疑问。
          </p>
        </div>

        {/* Card Grid */}
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          {CARDS.map((card) => (
            <button
              key={card.key}
              onClick={() => handleOpen(card)}
              className="group text-left bg-white rounded-xl border border-neutral-200 p-6 hover:border-primary-300 hover:shadow-md transition-all duration-150 flex flex-col gap-4"
            >
              {/* Icon */}
              <div className={`w-12 h-12 rounded-xl ${card.bg} flex items-center justify-center shrink-0`}>
                {card.icon}
              </div>

              {/* Content */}
              <div className="flex-1 space-y-1">
                <h2 className="text-base font-semibold text-neutral-800 group-hover:text-primary-500 transition-colors">
                  {card.title}
                </h2>
                <p className="text-sm text-neutral-500 leading-relaxed">
                  {card.desc}
                </p>
              </div>

              {/* Action */}
              <div className="flex items-center gap-1 text-sm font-medium text-primary-500">
                <span>{card.action}</span>
                {card.type === "external" ? (
                  <ExternalLink className="w-3.5 h-3.5" />
                ) : (
                  <ChevronRight className="w-3.5 h-3.5 group-hover:translate-x-0.5 transition-transform" />
                )}
              </div>
            </button>
          ))}
        </div>

        {/* Footer hint */}
        <p className="mt-8 text-center text-xs text-neutral-400">
          文档随版本持续更新 · 如有问题欢迎{" "}
          <button
            onClick={() => open("https://wanzij.cn/post-81.html")}
            className="text-primary-500 hover:underline"
          >
            反馈给作者
          </button>
        </p>
      </div>
    </div>
  );
}
