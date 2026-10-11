import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The page runs in the macOS WKWebView (mygo.config.ts:
// macos.minimumSystemVersion 13.0). strictPort keeps devUrl truthful.
export default defineConfig({
  plugins: [react()],
  server: { port: 5173, strictPort: true },
  build: { outDir: "dist", target: "safari15" },
});
