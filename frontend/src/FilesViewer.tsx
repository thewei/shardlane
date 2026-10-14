import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import githubMarkdownDark from "github-markdown-css/github-markdown-dark.css?raw";
import githubMarkdownLight from "github-markdown-css/github-markdown-light.css?raw";
import hljsGithubDark from "highlight.js/styles/github-dark.css?raw";
import { filesPreview, events, type FilesPreviewContent, type FilesPreviewTarget } from "./mygo";
import "./files-viewer.css";

/**
 * The Files preview body of the /files page, hosted by the native window
 * inside ui.WebView (product decision 2026-10-13, narrowed 2026-10-14).
 * The page renders only what HTML suits: Markdown (chat's markdown stack)
 * and images from the bridge's data URL. Text and code show in the native
 * code view and never reach this page; the viewer bar, size and the
 * Rendered/Source choice are native too. Strictly read-only: the page pulls
 * content through the filesPreview bridge and has no mutation channel —
 * create/rename/delete stay native, over the filesview domain.
 */

// GitHub's own markdown stylesheet, scoped to the page's data-theme: the
// light and dark variants are static stylesheets, so each one's selectors
// are prefixed to answer only under its theme attribute (the preview
// follows the app's appearance setting, not the system preference). The
// dark highlight tokens come from highlight.js's github-dark theme, scoped
// the same way — the global github.css light tokens keep covering light.
let markdownStylesInjected = false;
function injectGitHubMarkdownStyles() {
  if (markdownStylesInjected) return;
  markdownStylesInjected = true;
  const style = document.createElement("style");
  style.dataset.for = "viewer-github-markdown";
  style.textContent =
    githubMarkdownLight.replaceAll(
      ".markdown-body",
      '[data-theme="light"] .markdown-body',
    ) +
    "\n" +
    githubMarkdownDark.replaceAll(
      ".markdown-body",
      '[data-theme="dark"] .markdown-body',
    ) +
    "\n" +
    hljsGithubDark.replaceAll(".hljs", '[data-theme="dark"] .viewer-md .hljs');
  document.head.appendChild(style);
}
injectGitHubMarkdownStyles();

// Markdown renders with GitHub's own stylesheet and reading layout (GFM
// tables, task lists, fenced-code highlighting); raw HTML never passes
// through (react-markdown skips it). http(s) links leave through the
// bridge's OpenExternal gate; everything else stays inert.
function RenderedMarkdown({ content }: { content: FilesPreviewContent }) {
  return (
    <div className="viewer-scroll">
      <div className="markdown-body viewer-md">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          rehypePlugins={[rehypeHighlight]}
          components={{
            a: ({ href, children }) => (
              <a
                href={href}
                onClick={(event) => {
                  event.preventDefault();
                  if (href && /^https?:\/\//i.test(href)) {
                    void filesPreview.openExternal(href).catch(() => {});
                  }
                }}
              >
                {children}
              </a>
            ),
          }}
        >
          {content.content}
        </ReactMarkdown>
      </div>
    </div>
  );
}

export function FilesViewer() {
  const [target, setTarget] = useState<FilesPreviewTarget | null>(null);
  const [content, setContent] = useState<FilesPreviewContent | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [imageFailed, setImageFailed] = useState(false);
  const generation = useRef(0);

  // The selection arrives through the bridge event; the page starts from
  // Current so a reload keeps showing what the tree shows. An empty path
  // means the native view owns the file: the page lets go of its content.
  useEffect(() => {
    const off = events.filesPreview.on((next) => setTarget(next));
    filesPreview.current().then(setTarget).catch(() => {});
    return off;
  }, []);

  // Follow the app-resolved appearance carried by the bridge (and the
  // system preference until the first event lands), as the chat page does.
  const targetRef = useRef(target);
  targetRef.current = target;
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const theme = targetRef.current?.theme;
      document.documentElement.dataset.theme =
        theme ?? (media.matches ? "dark" : "light");
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);
  useEffect(() => {
    if (target?.theme) document.documentElement.dataset.theme = target.theme;
  }, [target?.theme]);

  const path = target?.path ?? "";
  useEffect(() => {
    const gen = ++generation.current;
    setContent(null);
    setError("");
    if (!path) return;
    setLoading(true);
    filesPreview
      .open(path)
      .then((next) => {
        if (gen === generation.current) setContent(next);
      })
      .catch((err: unknown) => {
        if (gen !== generation.current) return;
        setError(err instanceof Error ? err.message : "Read failed");
      })
      .finally(() => {
        if (gen === generation.current) setLoading(false);
      });
  }, [path]);

  const flavor = content?.flavor ?? "";
  const showMarkdown = Boolean(content && !content.isBinary && flavor === "markdown");
  const showImage = Boolean(content && flavor === "image" && content.dataUrl && !imageFailed);
  const showImageFailed = Boolean(content && flavor === "image" && imageFailed);

  // A corrupt or WKWebView-unsupported image breaks the <img> load; the
  // truthful state replaces the browser's broken-image glyph.
  useEffect(() => {
    setImageFailed(false);
  }, [content?.dataUrl]);

  return (
    <div className="viewer">
      <div className="viewer-body">
        {showMarkdown ? <RenderedMarkdown content={content!} /> : null}
        {showImage ? (
          <div className="viewer-image">
            <img
              src={content!.dataUrl}
              alt={content!.name}
              onError={() => setImageFailed(true)}
            />
          </div>
        ) : null}
        {showImageFailed ? (
          <div className="viewer-state">
            <p className="viewer-title">Can’t show this image</p>
            <p className="viewer-hint">Reveal it in Finder to open it.</p>
          </div>
        ) : null}
        {!showMarkdown && !showImage && !showImageFailed ? (
          <div className="viewer-state">
            {error ? (
              <>
                <p className="viewer-title">Can’t read this file</p>
                <p className="viewer-hint">{error}</p>
              </>
            ) : content?.isBinary ? (
              <>
                <p className="viewer-title">Binary file</p>
                <p className="viewer-hint">Reveal it in Finder to open it.</p>
              </>
            ) : loading ? (
              <p className="viewer-hint">Loading…</p>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
