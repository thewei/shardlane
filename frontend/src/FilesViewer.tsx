import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { search, searchKeymap } from "@codemirror/search";
import {
  HighlightStyle,
  StreamLanguage,
  syntaxHighlighting,
  type StreamParser,
} from "@codemirror/language";
import { properties } from "@codemirror/legacy-modes/mode/properties";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { tags as t } from "@lezer/highlight";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { filesPreview, events, type FilesPreviewContent, type FilesPreviewTarget } from "./mygo";
import "./files-viewer.css";

/**
 * The Files preview body of the /files page, hosted by the native window
 * inside ui.WebView (product decision 2026-10-13). Strictly read-only: the
 * page pulls content through the filesPreview bridge and has no mutation
 * channel — create/rename/delete stay native, over the filesview domain.
 * Markdown renders by default (chat's markdown stack) with a source
 * toggle; images render from the bridge's data URL. Files the WebView tier
 * cannot take preview natively and never reach this page.
 */

// Read-only highlight vocabulary, themed through CSS classes so both
// appearances follow the app tokens without reconfiguring the editor.
const filesHighlight = HighlightStyle.define([
  { tag: t.keyword, class: "cmk-keyword" },
  { tag: [t.controlKeyword, t.moduleKeyword], class: "cmk-keyword" },
  { tag: [t.function(t.variableName), t.labelName, t.function(t.propertyName)], class: "cmk-func" },
  { tag: [t.typeName, t.className, t.namespace], class: "cmk-type" },
  { tag: [t.number, t.bool, t.atom, t.special(t.variableName), t.constant(t.name), t.standard(t.name)], class: "cmk-num" },
  { tag: [t.string, t.special(t.string), t.processingInstruction, t.inserted], class: "cmk-string" },
  { tag: [t.operator, t.operatorKeyword, t.punctuation, t.bracket], class: "cmk-op" },
  { tag: [t.comment, t.lineComment, t.blockComment], class: "cmk-comment", fontStyle: "italic" },
  { tag: [t.meta, t.annotation], class: "cmk-comment" },
  { tag: [t.regexp, t.escape, t.url], class: "cmk-escape" },
  { tag: [t.variableName, t.propertyName, t.definition(t.name)], class: "cmk-name" },
  { tag: t.heading, fontWeight: "bold" },
  { tag: t.strong, fontWeight: "bold" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: t.link, textDecoration: "underline" },
  { tag: t.invalid, class: "cmk-invalid" },
]);

// editors extensions for one file: a read-only code surface with line
// numbers, in-page search (⌘F) and the themed highlight style.
function editorExtensions(lang: Extension | null): Extension[] {
  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    EditorView.editable.of(false),
    EditorState.readOnly.of(true),
    keymap.of([...defaultKeymap, ...searchKeymap]),
    search({ top: true }),
    syntaxHighlighting(filesHighlight),
  ];
  if (lang) {
    extensions.push(lang);
  }
  return extensions;
}

// languageFor maps a filename to a language package. Language packages are
// dynamically imported, so the initial route pays only for the editor
// core; the three tiny legacy grammars are static (a few KB). Unknown
// extensions and loader failures degrade to plain text.
type LangLoader = () => Promise<Extension>;

const javascript = (opts?: { jsx?: boolean; typescript?: boolean }): LangLoader =>
  () => import("@codemirror/lang-javascript").then((m) => m.javascript(opts));
const legacy = <P extends StreamParser<unknown>>(parser: P): LangLoader =>
  () => Promise.resolve(StreamLanguage.define(parser));

const langLoaders: Record<string, LangLoader> = {
  js: javascript({ jsx: true }),
  jsx: javascript({ jsx: true }),
  mjs: javascript({ jsx: true }),
  cjs: javascript({ jsx: true }),
  ts: javascript({ typescript: true, jsx: true }),
  tsx: javascript({ typescript: true, jsx: true }),
  go: () => import("@codemirror/lang-go").then((m) => m.go()),
  py: () => import("@codemirror/lang-python").then((m) => m.python()),
  pyw: () => import("@codemirror/lang-python").then((m) => m.python()),
  rs: () => import("@codemirror/lang-rust").then((m) => m.rust()),
  json: () => import("@codemirror/lang-json").then((m) => m.json()),
  jsonc: () => import("@codemirror/lang-json").then((m) => m.json()),
  md: () => import("@codemirror/lang-markdown").then((m) => m.markdown()),
  markdown: () => import("@codemirror/lang-markdown").then((m) => m.markdown()),
  html: () => import("@codemirror/lang-html").then((m) => m.html()),
  htm: () => import("@codemirror/lang-html").then((m) => m.html()),
  vue: () => import("@codemirror/lang-html").then((m) => m.html()),
  css: () => import("@codemirror/lang-css").then((m) => m.css()),
  scss: () => import("@codemirror/lang-css").then((m) => m.css()),
  less: () => import("@codemirror/lang-css").then((m) => m.css()),
  yml: () => import("@codemirror/lang-yaml").then((m) => m.yaml()),
  yaml: () => import("@codemirror/lang-yaml").then((m) => m.yaml()),
  c: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  h: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  cpp: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  hpp: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  cc: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  m: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  java: () => import("@codemirror/lang-java").then((m) => m.java()),
  php: () => import("@codemirror/lang-php").then((m) => m.php()),
  sql: () => import("@codemirror/lang-sql").then((m) => m.sql()),
  sh: legacy(shell),
  bash: legacy(shell),
  zsh: legacy(shell),
  toml: legacy(toml),
  conf: legacy(toml),
  properties: legacy(properties),
};

function languageFor(name: string): Promise<Extension | null> {
  const dot = name.lastIndexOf(".");
  if (dot === -1) return Promise.resolve(null);
  const loader = langLoaders[name.slice(dot + 1).toLowerCase()];
  if (!loader) return Promise.resolve(null);
  return loader().catch(() => null);
}

// Markdown markdown renders with the chat page's exact stack and `.md`
// styles (GFM tables, fenced-code highlighting); raw HTML never passes
// through (react-markdown skips it). http(s) links leave through the
// bridge's OpenExternal gate; everything else stays inert.
function RenderedMarkdown({ content }: { content: FilesPreviewContent }) {
  return (
    <div className="viewer-scroll">
      <div className="md viewer-md">
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
  // Markdown defaults to the rendered view; the toggle is a page-level
  // preference and stays sticky across files.
  const [markdownRendered, setMarkdownRendered] = useState(true);
  const [imageFailed, setImageFailed] = useState(false);
  const generation = useRef(0);
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const showEditor = Boolean(
    content &&
      !content.isBinary &&
      content.flavor !== "image" &&
      (content.flavor !== "markdown" || !markdownRendered),
  );

  // The tree selection arrives through the bridge event; the page starts
  // from Current so a reload keeps showing what the tree shows.
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

  // The editor is created once its host exists (first file with content);
  // every later file swaps the state (fresh document, fresh language),
  // which also resets scroll per file.
  useEffect(() => {
    if (!showEditor || viewRef.current || !hostRef.current) return;
    const view = new EditorView({
      parent: hostRef.current,
      state: EditorState.create({ doc: "", extensions: editorExtensions(null) }),
    });
    viewRef.current = view;
    return () => {
      if (viewRef.current === view) viewRef.current = null;
      view.destroy();
    };
  }, [showEditor]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !content || content.isBinary || content.flavor === "image") return;
    let cancelled = false;
    void languageFor(content.name).then((lang) => {
      if (cancelled || viewRef.current !== view) return;
      view.setState(
        EditorState.create({
          doc: content.content,
          extensions: editorExtensions(lang),
        }),
      );
    });
    return () => {
      cancelled = true;
    };
  }, [content, showEditor]);

  const flavor = content?.flavor ?? "";
  const showMarkdown = Boolean(
    content && !content.isBinary && flavor === "markdown" && markdownRendered,
  );
  const showImage = Boolean(content && flavor === "image" && content.dataUrl && !imageFailed);
  const showImageFailed = Boolean(content && flavor === "image" && imageFailed);

  // A corrupt or WKWebView-unsupported image breaks the <img> load; the
  // truthful state replaces the browser's broken-image glyph.
  useEffect(() => {
    setImageFailed(false);
  }, [content?.dataUrl]);

  return (
    <div className="viewer">
      <div className="viewer-bar">
        {target?.name ? (
          <>
            <span className="viewer-name" title={target.path}>
              {target.name}
            </span>
            {content && content.sizeBytes > 0 ? (
              <span className="viewer-size">{formatSize(content.sizeBytes)}</span>
            ) : null}
            {content?.truncated && flavor !== "image" ? (
              <span className="pill">Preview limited to 1 MiB</span>
            ) : null}
            {loading ? <span className="viewer-loading">Loading…</span> : null}
            {content && !content.isBinary && flavor === "markdown" ? (
              <span className="viewer-seg" role="tablist" aria-label="Markdown view">
                <button
                  type="button"
                  className={markdownRendered ? "pick" : ""}
                  onClick={() => setMarkdownRendered(true)}
                >
                  Rendered
                </button>
                <button
                  type="button"
                  className={!markdownRendered ? "pick" : ""}
                  onClick={() => setMarkdownRendered(false)}
                >
                  Source
                </button>
              </span>
            ) : null}
          </>
        ) : (
          <span className="viewer-muted">No file selected</span>
        )}
      </div>
      <div className="viewer-body">
        {showEditor ? <div className="viewer-editor" ref={hostRef} /> : null}
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
            <p className="viewer-hint">
              {target?.name} can’t be rendered here. Use “Reveal in Finder” above to open it.
            </p>
          </div>
        ) : null}
        {!showEditor && !showMarkdown && !showImage && !showImageFailed ? (
          <div className="viewer-state">
            {loading && !target?.name ? (
              <p className="viewer-hint">Loading…</p>
            ) : error ? (
              <>
                <p className="viewer-title">Can’t read this file</p>
                <p className="viewer-hint">{error}</p>
              </>
            ) : content?.isBinary ? (
              <>
                <p className="viewer-title">Binary file</p>
                <p className="viewer-hint">
                  {target?.name} isn’t text. Use “Reveal in Finder” above to open it.
                </p>
              </>
            ) : target?.name ? (
              <p className="viewer-hint">Loading…</p>
            ) : (
              <>
                <p className="viewer-title">Select a file to preview</p>
                <p className="viewer-hint">
                  Click a file in the sidebar. Press ⌘F to search inside once open.
                </p>
              </>
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}
