import {
  Github,
  Mail,
  Star,
  Code2,
  Briefcase,
  Heart,
  ExternalLink,
} from "lucide-react";

const SKILLS = [
  { name: "PHP", color: "bg-purple-100 text-purple-700" },
  { name: "Python", color: "bg-blue-100 text-blue-700" },
  { name: "C#", color: "bg-green-100 text-green-700" },
  { name: "JavaScript", color: "bg-yellow-100 text-yellow-800" },
  { name: "CSS", color: "bg-pink-100 text-pink-700" },
  { name: "Vue 2 / 3", color: "bg-emerald-100 text-emerald-700" },
  { name: "微信小程序", color: "bg-teal-100 text-teal-700" },
  { name: "UniApp", color: "bg-sky-100 text-sky-700" },
];

const DOMAINS = [
  { icon: "🌾", label: "农业物联网业务系统" },
  { icon: "🛒", label: "零售电商业务系统" },
  { icon: "📚", label: "教培网课业务系统" },
];

export function AboutPage() {
  return (
    <div className="min-h-full bg-neutral-50 px-6 py-8">
      <div className="max-w-2xl mx-auto space-y-6">

        {/* Header Card */}
        <div className="bg-white rounded-xl border border-neutral-200 p-6 flex items-center gap-5">
          <div className="w-16 h-16 rounded-full bg-gradient-to-br from-primary-400 to-primary-600 flex items-center justify-center shrink-0 shadow-sm">
            <span className="text-2xl font-bold text-white select-none">W</span>
          </div>
          <div>
            <h1 className="text-lg font-semibold text-neutral-800">独立开发者 · 后端工程师</h1>
            <p className="mt-1 text-sm text-neutral-500 leading-relaxed">
              深耕互联网开发 <span className="font-medium text-primary-500">9 年</span>，对技术和开源充满热情。
              从无 AI 到有 AI 时代，始终乐于折腾、乐于分享。
            </p>
          </div>
        </div>

        {/* Experience */}
        <div className="bg-white rounded-xl border border-neutral-200 p-6 space-y-4">
          <div className="flex items-center gap-2 text-neutral-700 font-medium">
            <Briefcase className="w-4 h-4 text-primary-500" />
            <span>行业经验</span>
          </div>
          <div className="grid grid-cols-1 gap-3">
            {DOMAINS.map((d) => (
              <div
                key={d.label}
                className="flex items-center gap-3 px-4 py-3 rounded-lg bg-neutral-50 border border-neutral-100"
              >
                <span className="text-xl">{d.icon}</span>
                <span className="text-sm text-neutral-700">{d.label}</span>
              </div>
            ))}
          </div>
        </div>

        {/* Skills */}
        <div className="bg-white rounded-xl border border-neutral-200 p-6 space-y-4">
          <div className="flex items-center gap-2 text-neutral-700 font-medium">
            <Code2 className="w-4 h-4 text-primary-500" />
            <span>技术栈</span>
          </div>
          <div className="flex flex-wrap gap-2">
            {SKILLS.map((s) => (
              <span
                key={s.name}
                className={`px-3 py-1 rounded-full text-xs font-medium ${s.color}`}
              >
                {s.name}
              </span>
            ))}
          </div>
        </div>

        {/* Support */}
        <div className="bg-white rounded-xl border border-neutral-200 p-6 space-y-5">
          <div className="flex items-center gap-2 text-neutral-700 font-medium">
            <Heart className="w-4 h-4 text-red-400" />
            <span>支持我</span>
          </div>

          {/* GitHub */}
          <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-4 space-y-2">
            <div className="flex items-center gap-2">
              <Github className="w-4 h-4 text-neutral-600" />
              <span className="text-sm font-medium text-neutral-700">GitHub Star</span>
            </div>
            <p className="text-sm text-neutral-500 leading-relaxed">
              如果这个项目对你有帮助，欢迎给仓库点个 ⭐，这对我找工作也有很大帮助！
            </p>
            <a
              href="https://github.com/wzj177/agent-claw-box"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 mt-1 text-sm text-primary-500 hover:text-primary-600 font-medium transition-colors"
            >
              <Star className="w-3.5 h-3.5" />
              github.com/wzj177/agent-claw-box
              <ExternalLink className="w-3 h-3 opacity-70" />
            </a>
            <a
              href="https://github.com/wzj177"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 text-sm text-neutral-500 hover:text-primary-500 transition-colors"
            >
              <Github className="w-3.5 h-3.5" />
              更多开源项目见 github.com/wzj177
              <ExternalLink className="w-3 h-3 opacity-70" />
            </a>
          </div>

          {/* Email */}
          <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-4 space-y-2">
            <div className="flex items-center gap-2">
              <Mail className="w-4 h-4 text-neutral-600" />
              <span className="text-sm font-medium text-neutral-700">外包合作 / 业务咨询</span>
            </div>
            <p className="text-sm text-neutral-500 leading-relaxed">
              有外包项目开发需求欢迎联系，主要承接后端、全栈类项目。
            </p>
            <a
              href="mailto:wanzij177@163.com"
              className="inline-flex items-center gap-1.5 mt-1 text-sm text-primary-500 hover:text-primary-600 font-medium transition-colors"
            >
              <Mail className="w-3.5 h-3.5" />
              wanzij177@163.com
            </a>
          </div>
        </div>

        {/* Hobby */}
        <div className="bg-white rounded-xl border border-neutral-200 px-6 py-4 flex items-center gap-3">
          <span className="text-xl">🎮</span>
          <p className="text-sm text-neutral-500">
            开发之余喜欢打打<span className="text-neutral-700 font-medium">王者荣耀</span>，欢迎一起上分～
          </p>
        </div>

      </div>
    </div>
  );
}
