import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "highlight.js/styles/github.css";
import "./theme.css";

// One embedded SPA serves both WebView pages from the app content origin:
// /chat (the chat body) and /files-viewer (the Files preview body). The
// FilesViewer chunk is lazy, so the chat route never pays for CodeMirror.
const FilesViewer = React.lazy(() =>
  import("./FilesViewer").then((m) => ({ default: m.FilesViewer })),
);

const isFilesViewer = /\/files-viewer\/?$/.test(window.location.pathname);

// Dev-only crash visor: an embedded WKWebView has no inspector in the
// acceptance loop, so surface fatal page errors on the page itself.
if (import.meta.env.DEV) {
  const show = (message: string) => {
    let host = document.getElementById("crash-visor");
    if (!host) {
      host = document.createElement("div");
      host.id = "crash-visor";
      host.style.cssText =
        "position:fixed;inset:auto 12px 12px 12px;z-index:99999;background:#ff3b30;color:#fff;" +
        "font:12px/1.5 ui-monospace,Menlo,monospace;padding:10px;border-radius:8px;white-space:pre-wrap;max-height:40%;overflow:auto";
      document.body.appendChild(host);
    }
    host.textContent += `${message}\n`;
  };
  window.addEventListener("error", (e) => show(`error: ${e.message}\n${e.error?.stack ?? ""}`));
  window.addEventListener("unhandledrejection", (e) =>
    show(`rejection: ${e.reason instanceof Error ? `${e.reason.message}\n${e.reason.stack}` : String(e.reason)}`),
  );
}

const root = createRoot(document.getElementById("root")!);
root.render(
  <React.StrictMode>
    {isFilesViewer ? (
      <React.Suspense fallback={<div style={{ padding: 16, color: "#888" }}>Loading viewer…</div>}>
        <FilesViewer />
      </React.Suspense>
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
