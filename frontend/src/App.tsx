import React, {
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
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
  const composingRef = useRef(false);
  const logRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

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

  // Follow the system appearance so the body sits seamlessly under the
  // native header band.
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      document.documentElement.dataset.theme = media.matches ? "dark" : "light";
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);

  // Sticky bottom: follow the tail until the user scrolls up.
  useEffect(() => {
    const log = logRef.current;
    if (log && stickToBottom.current) {
      log.scrollTop = log.scrollHeight;
    }
  }, [state]);

  const onLogScroll = () => {
    const log = logRef.current;
    if (!log) return;
    stickToBottom.current = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
  };

  const disposition = state?.disposition ?? "unknown";
  const canSend =
    disposition === "ready" && draft.trim() !== "" && !sending && !state?.loading;

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
            <TurnRow key={turnKey(turn, index)} turn={turn} />
          ))
        )}
      </div>
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
            value={draft}
            placeholder="Follow up on this conversation…"
            aria-label="Chat prompt"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            onCompositionStart={() => (composingRef.current = true)}
            onCompositionEnd={() => (composingRef.current = false)}
          />
          <button
            type="button"
            className="primary"
            disabled={!canSend}
            onClick={() => void send()}
          >
            Send
          </button>
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
      {turn.narration ? (
        <div className="narration">{turn.narration}</div>
      ) : null}
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
