/// <reference types="vite/client" />

// 允许 TypeScript 识别 ?raw 导入（Vite 内置，返回文件原始字符串内容）
declare module "*.md?raw" {
  const content: string;
  export default content;
}
