import { useParams, useNavigate } from "react-router-dom";
import { useEffect, useState } from "react";
import { marked } from "marked";
import { ArrowLeft } from "lucide-react";

// 在构建时将 Markdown 文件内联为字符串，打包后完全离线可用
import faqRaw from "@docs/FAQ.md?raw";
import guideRaw from "@docs/GUIDE.md?raw";

const DOCS: Record<string, { title: string; content: string }> = {
  faq: { title: "常见问题", content: faqRaw },
  guide: { title: "使用教程", content: guideRaw },
};

export function DocViewerPage() {
  const { slug } = useParams<{ slug: string }>();
  const navigate = useNavigate();
  const [html, setHtml] = useState("");

  const doc = slug ? DOCS[slug] : undefined;

  useEffect(() => {
    if (!doc) return;
    // marked.parse 在未使用异步扩展时返回 string
    const result = marked.parse(doc.content);
    if (typeof result === "string") {
      setHtml(result);
    } else {
      result.then(setHtml);
    }
  }, [doc]);

  if (!doc) {
    return (
      <div className="min-h-full flex items-center justify-center">
        <p className="text-neutral-400">文档不存在</p>
      </div>
    );
  }

  return (
    <div className="min-h-full bg-neutral-50">
      {/* Toolbar */}
      <div className="sticky top-0 z-10 bg-white border-b border-neutral-200 px-6 py-3 flex items-center gap-3">
        <button
          onClick={() => navigate("/help")}
          className="flex items-center gap-1.5 text-sm text-neutral-500 hover:text-neutral-800 transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          帮助中心
        </button>
        <span className="text-neutral-300">/</span>
        <span className="text-sm font-medium text-neutral-700">{doc.title}</span>
      </div>

      {/* Markdown content */}
      <div className="max-w-3xl mx-auto px-6 py-8">
        <div
          className="prose prose-neutral prose-sm max-w-none
            [&_h1]:text-xl [&_h1]:font-bold [&_h1]:text-neutral-800 [&_h1]:mb-4 [&_h1]:mt-0
            [&_h2]:text-lg [&_h2]:font-semibold [&_h2]:text-neutral-800 [&_h2]:mt-8 [&_h2]:mb-3 [&_h2]:pb-2 [&_h2]:border-b [&_h2]:border-neutral-200
            [&_h3]:text-base [&_h3]:font-semibold [&_h3]:text-neutral-700 [&_h3]:mt-6 [&_h3]:mb-2
            [&_p]:text-neutral-600 [&_p]:leading-relaxed [&_p]:my-2
            [&_a]:text-primary-500 [&_a]:no-underline hover:[&_a]:underline
            [&_code]:bg-neutral-100 [&_code]:text-pink-600 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs
            [&_pre]:bg-neutral-900 [&_pre]:text-neutral-100 [&_pre]:rounded-lg [&_pre]:p-4 [&_pre]:overflow-x-auto [&_pre]:my-4
            [&_pre_code]:bg-transparent [&_pre_code]:text-inherit [&_pre_code]:p-0
            [&_ul]:list-disc [&_ul]:pl-5 [&_ul]:my-2 [&_ul]:text-neutral-600
            [&_ol]:list-decimal [&_ol]:pl-5 [&_ol]:my-2 [&_ol]:text-neutral-600
            [&_li]:my-1
            [&_table]:w-full [&_table]:border-collapse [&_table]:my-4
            [&_th]:bg-neutral-100 [&_th]:text-left [&_th]:px-3 [&_th]:py-2 [&_th]:text-sm [&_th]:font-medium [&_th]:border [&_th]:border-neutral-200
            [&_td]:px-3 [&_td]:py-2 [&_td]:text-sm [&_td]:text-neutral-600 [&_td]:border [&_td]:border-neutral-200
            [&_hr]:border-neutral-200 [&_hr]:my-6
            [&_blockquote]:border-l-4 [&_blockquote]:border-primary-300 [&_blockquote]:pl-4 [&_blockquote]:text-neutral-500 [&_blockquote]:italic [&_blockquote]:my-4
            [&_strong]:text-neutral-800 [&_strong]:font-semibold"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      </div>
    </div>
  );
}
