import React, {
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { chat, events, type ChatBridgeState } from "./mygo";
import "./chat.css";

/**
 * The Chat body of the /workspace WorkspacePrimarySurface, hosted by the
 * native window inside ui.WebView. The conversation runtime, send path and
 * queue stay in Go (internal/agent + internal/herdr): this page renders the
 * bridge projection and calls the bound service — presentation only.
 */

// The bridge mirrors conversation.PromptDisposition as "ready" | "queue" |
// "terminal" | "unknown"; the generated client types it as string.
export function App() {
  const [state, setState] = useState<ChatBridgeState | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [showJump, setShowJump] = useState(false);
  const composingRef = useRef(false);
  const logRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const stickToBottom = useRef(true);
  const turnsRef = useRef(0);

  const refresh = useCallback(() => {
    chat.snapshot().then(setState).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    // Events emitted before the DOM was ready are replayed by the runtime;
    // every later projection change arrives through chatUpdated.
    const off = events.chatUpdated.on(setState);
    return off;
  }, [refresh]);

  // Follow the app-resolved appearance (Settings -> General -> Appearance,
  // carried by the bridge state once it arrives); until then, the system
  // preference.
  const stateRef = useRef(state);
  stateRef.current = state;
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const theme = stateRef.current?.theme;
      document.documentElement.dataset.theme =
        theme ?? (media.matches ? "dark" : "light");
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);
  useEffect(() => {
    if (state?.theme) {
      document.documentElement.dataset.theme = state.theme;
    }
  }, [state?.theme]);

  // Sticky bottom: follow the tail until the user scrolls up; turns added
  // while scrolled up raise the "New messages" pill instead.
  useEffect(() => {
    const log = logRef.current;
    const count = state?.turns.length ?? 0;
    if (log && stickToBottom.current) {
      log.scrollTop = log.scrollHeight;
      setShowJump(false);
    } else if (count > turnsRef.current) {
      setShowJump(true);
    }
    turnsRef.current = count;
  }, [state]);

  const onLogScroll = () => {
    const log = logRef.current;
    if (!log) return;
    const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
    stickToBottom.current = atBottom;
    if (atBottom) setShowJump(false);
  };

  const jumpToLatest = () => {
    const log = logRef.current;
    if (!log) return;
    stickToBottom.current = true;
    setShowJump(false);
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    log.scrollTo({ top: log.scrollHeight, behavior: reduced ? "auto" : "smooth" });
  };

  // Drafts persist per conversation across pane switches and page reloads
  // (session storage, keyed by conversation id).
  const conversationId = state?.conversationId ?? "";
  useEffect(() => {
    if (!conversationId) return;
    setDraft(sessionStorage.getItem(draftKey(conversationId)) ?? "");
  }, [conversationId]);
  useEffect(() => {
    if (conversationId) sessionStorage.setItem(draftKey(conversationId), draft);
  }, [draft, conversationId]);

  // Auto-grow the composer up to the viewport cap.
  useEffect(() => {
    const el = composerRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, Math.floor(window.innerHeight * 0.4))}px`;
  }, [draft]);

  const disposition = state?.disposition ?? "unknown";
  const working = disposition === "queue";
  const canSend =
    disposition === "ready" && draft.trim() !== "" && !sending && !state?.loading;
  const canQueue = working && draft.trim() !== "" && !sending;
  // One composer action, as in the reference chat: while the agent works
  // with an empty box it is Stop (one Escape through the bridge); typing
  // swaps it back to Queue/Send.
  const showStop = working && draft.trim() === "" && !sending;

  const send = async () => {
    const text = draft.trim();
    if (text === "" || sending) return;
    setSending(true);
    try {
      const next = await chat.send(text);
      setState(next);
      if (!next.outcome) {
        setDraft("");
        stickToBottom.current = true;
      }
    } catch {
      // The bridge keeps the draft on hard failures; the outcome line in the
      // next projection explains what happened.
      refresh();
    } finally {
      setSending(false);
    }
  };

  const stop = async () => {
    setSending(true);
    try {
      setState(await chat.stop());
    } catch {
      refresh();
    } finally {
      setSending(false);
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter sends on the ready disposition (chat convention); IME
    // composition never sends — a half-typed candidate must not commit.
    if (event.key !== "Enter" || event.shiftKey) return;
    if (composingRef.current || event.nativeEvent.isComposing) return;
    event.preventDefault();
    if (canSend) void send();
  };

  const cancelQueued = async () => {
    setState(await chat.cancelQueued().catch(() => state));
  };

  return (
    <div className="chat">
      <div className="chat-log" ref={logRef} onScroll={onLogScroll}>
        {state?.loading ? (
          <div className="chat-note">Loading conversation…</div>
        ) : !state || state.turns.length === 0 ? (
          <div className="chat-note">No conversation yet.</div>
        ) : (
          state.turns.map((turn, index) => (
            <TurnBoundary key={turnKey(turn, index)}>
              <TurnRow turn={turn} />
            </TurnBoundary>
          ))
        )}
      </div>
      {showJump ? (
        <button type="button" className="jump" onClick={jumpToLatest}>
          New messages ↓
        </button>
      ) : null}
      <div className="composer">
        {state?.outcome ? <div className="outcome">{state.outcome}</div> : null}
        {state?.queuedText ? (
          <div className="queued">
            <span>Queued after turn</span>
            <button type="button" onClick={cancelQueued}>
              Cancel
            </button>
          </div>
        ) : null}
        <div className="composer-row">
          <textarea
            ref={composerRef}
            value={draft}
            placeholder="Follow up on this conversation…"
            aria-label="Chat prompt"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            onCompositionStart={() => (composingRef.current = true)}
            onCompositionEnd={() => (composingRef.current = false)}
          />
          {showStop ? (
            <button type="button" className="primary stop" onClick={() => void stop()}>
              Stop
            </button>
          ) : (
            <button
              type="button"
              className="primary"
              disabled={!(canSend || canQueue)}
              onClick={() => void send()}
            >
              {working ? "Queue" : "Send"}
            </button>
          )}
        </div>
        {disposition === "terminal" ? (
          <div className="composer-note">Blocked — use the terminal.</div>
        ) : disposition === "queue" ? (
          <div className="composer-note">Working — the prompt will queue.</div>
        ) : null}
      </div>
    </div>
  );
}

function turnKey(turn: ChatBridgeState["turns"][number], index: number): string {
  return `${index}:${turn.user ? "u" : "a"}`;
}

function draftKey(conversationId: string): string {
  return `shardlane:chat-draft:${conversationId}`;
}

// TurnBoundary contains one turn's render failures: a bad turn renders as
// an inline notice and the rest of the transcript keeps working.
class TurnBoundary extends React.Component<
  { children: React.ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) {
      return <div className="turn-error">This message can't be shown.</div>;
    }
    return this.props.children;
  }
}

// Markdown renders assistant narration (GFM tables/lists, fenced code with
// highlighting). Raw HTML never passes through: react-markdown skips it.
const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          pre: (props) => <CodeBlock {...props} />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});

function CodeBlock({ children, ...rest }: React.HTMLAttributes<HTMLPreElement>) {
  const ref = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = () => {
    const text = ref.current?.textContent ?? "";
    if (text === "") return;
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <div className="codeblock">
      <button type="button" className="code-copy" onClick={copy}>
        {copied ? "Copied" : "Copy"}
      </button>
      <pre ref={ref} {...rest}>
        {children}
      </pre>
    </div>
  );
}

function TurnRow({ turn }: { turn: ChatBridgeState["turns"][number] }) {
  if (turn.user) {
    return (
      <div className="turn user">
        <div className="bubble">{turn.text}</div>
      </div>
    );
  }
  return (
    <div className="turn assistant">
      <div className="meta">
        <span className="who">ASSISTANT</span>
        {turn.failed ? <span className="pill error">Stopped</span> : null}
      </div>
      {turn.narration ? <Markdown text={turn.narration} /> : null}
      {turn.thinking ? (
        <details className="block">
          <summary>Thinking</summary>
          <div className="block-body muted">{turn.thinking}</div>
        </details>
      ) : null}
      {turn.tools?.length ? (
        <details className="block" open={turn.tools.length === 1}>
          <summary>
            {turn.compacted ? "Tool activity (grouped)" : "Tool activity"} (
            {turn.tools.length})
          </summary>
          <div className="block-body">
            {turn.tools.map((tool, index) => (
              <details
                key={index}
                className={tool.isError ? "tool error" : "tool"}
              >
                <summary>{tool.name}</summary>
                {tool.input ? <pre>{tool.input}</pre> : null}
                {tool.output ? <pre>{tool.output}</pre> : null}
              </details>
            ))}
          </div>
        </details>
      ) : null}
    </div>
  );
}
