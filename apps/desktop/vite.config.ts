import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      // docs/ 目录别名，供 ?raw 内联导入
      "@docs": path.resolve(__dirname, "../../docs"),
    },
  },
  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    fs: {
      allow: [path.resolve(__dirname, "../..")],
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
