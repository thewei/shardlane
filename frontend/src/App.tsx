import React, {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
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
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [completions, setCompletions] = useState<string[]>([]);
  const [completionPick, setCompletionPick] = useState(0);
  const [recording, setRecording] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [voiceError, setVoiceError] = useState("");
  const [interrupting, setInterrupting] = useState(false);
  const [textPrompt, setTextPrompt] = useState<{ id: string; option: string } | null>(null);
  const [textAnswer, setTextAnswer] = useState("");
  const [answering, setAnswering] = useState(false);
  const composingRef = useRef(false);
  const logRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const meterRef = useRef<HTMLCanvasElement>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const meterRafRef = useRef(0);
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

  // Prepend anchoring: when a history page lands above, keep the viewport
  // pinned to the messages the user was reading.
  const headRef = useRef("");
  const heightRef = useRef(0);
  useLayoutEffect(() => {
    const log = logRef.current;
    if (!log) return;
    const first = state?.turns[0];
    const head = first ? `${first.user ? "u" : "a"}:${first.tsMs ?? ""}:${(first.text ?? first.narration ?? "").slice(0, 32)}` : "";
    if (headRef.current && head !== headRef.current && log.scrollHeight > heightRef.current) {
      log.scrollTop += log.scrollHeight - heightRef.current;
    }
    headRef.current = head;
    heightRef.current = log.scrollHeight;
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

  // @ completion: when the draft ends in an @token (no whitespace after
  // the last "@"), suggest the pane project's matching entries.
  useEffect(() => {
    const at = draft.lastIndexOf("@");
    if (at === -1) {
      setCompletions([]);
      return;
    }
    const token = draft.slice(at + 1);
    if (/\s/.test(token)) {
      setCompletions([]);
      return;
    }
    const timer = window.setTimeout(() => {
      chat.listFiles(token)
        .then((list) => {
          setCompletions(list ?? []);
          setCompletionPick(0);
        })
        .catch(() => setCompletions([]));
    }, 120);
    return () => window.clearTimeout(timer);
  }, [draft]);

  const acceptCompletion = (index: number) => {
    const at = draft.lastIndexOf("@");
    if (at === -1 || completions[index] === undefined) return;
    setDraft(`${draft.slice(0, at + 1)}${completions[index]} `);
    setCompletions([]);
    composerRef.current?.focus();
  };

  // Auto-grow the composer up to the viewport cap.
  useEffect(() => {
    const el = composerRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, Math.floor(window.innerHeight * 0.4))}px`;
  }, [draft]);

  const disposition = state?.disposition ?? "unknown";
  const working = disposition === "queue";
  // The waiting placeholder stands in for the assistant card between the
  // user's sent row and the first live tail content (the agent's own TUI
  // shows the same "waiting → thinking → working" progression).
  const lastTurn = state?.turns[state.turns.length - 1];
  const showPendingAssistant = Boolean(
    working && !state?.loading && lastTurn && lastTurn.user,
  );
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

  const answerInteraction = async (id: string, option: string) => {
    if (isFreeTextOption(option)) {
      setTextPrompt({ id, option });
      setTextAnswer("");
      return;
    }
    try {
      setState(await chat.answerInteraction(id, option));
    } catch {
      refresh(); // lost a CAS race or the prompt moved on: re-read
    }
  };

  // submitTextAnswer delivers the typed answer for a free-text option: the
  // bridge selects the option, settles for the TUI's text field, then pastes
  // and submits the text through agent.prompt.
  const submitTextAnswer = async () => {
    const text = textAnswer.trim();
    if (!textPrompt || text === "" || answering) return;
    setAnswering(true);
    try {
      setState(await chat.answerInteractionText(textPrompt.id, textPrompt.option, text));
      setTextPrompt(null);
      setTextAnswer("");
    } catch {
      refresh(); // lost a race or the delivery failed: re-read
    } finally {
      setAnswering(false);
    }
  };

  const onKeyDownAnswerText = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (composingRef.current || event.nativeEvent.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submitTextAnswer();
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setTextPrompt(null);
      setTextAnswer("");
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

  // interruptSend is the explicit cut-in: the bridge stops the running turn
  // (one Escape), settles briefly, then delivers this text now instead of
  // queueing it behind the turn.
  const interruptSend = async () => {
    const text = draft.trim();
    if (text === "" || interrupting) return;
    setInterrupting(true);
    try {
      const next = await chat.interruptAndSend(text);
      setState(next);
      if (!next.outcome) {
        setDraft("");
        stickToBottom.current = true;
      }
    } catch {
      refresh();
    } finally {
      setInterrupting(false);
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // The @ completion list owns navigation keys while it is open.
    if (completions.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setCompletionPick((pick) => (pick + 1) % completions.length);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setCompletionPick((pick) => (pick - 1 + completions.length) % completions.length);
        return;
      }
      if (event.key === "Tab" || event.key === "Enter") {
        event.preventDefault();
        if (!event.nativeEvent.isComposing) acceptCompletion(completionPick);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setCompletions([]);
        return;
      }
    }
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

  const loadOlder = async () => {
    if (loadingOlder) return;
    setLoadingOlder(true);
    try {
      await chat.older();
      // The refreshed projection (and the anchoring effect) arrive through
      // the chat:updated event.
    } finally {
      setLoadingOlder(false);
    }
  };

  // insertAtCaret splices text into the draft at the caret (attachments,
  // dictated text) and puts the caret after the insertion.
  const insertAtCaret = useCallback((inserted: string) => {
    const el = composerRef.current;
    if (!el) {
      setDraft((current) => current + inserted);
      return;
    }
    const start = el.selectionStart ?? draft.length;
    const end = el.selectionEnd ?? draft.length;
    setDraft(draft.slice(0, start) + inserted + draft.slice(end));
    requestAnimationFrame(() => {
      el.focus();
      const caret = start + inserted.length;
      el.setSelectionRange(caret, caret);
    });
  }, [draft]);

  const attach = async () => {
    const path = await chat.pickAttachment().catch(() => "");
    if (!path) return;
    insertAtCaret(`@${path} `);
  };

  // Dictation: record in the page (WKWebView, secure context), then the
  // bridge forwards the clip to the configured OpenAI-compatible endpoint.
  // While recording, an AnalyserNode feeds the small level meter (the
  // reference chat's live visualizer) so the mic state is visible evidence,
  // not a pulsing button alone.
  const startDictation = async () => {
    setVoiceError("");
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const audioCtx = new AudioContext();
      if (audioCtx.state === "suspended") void audioCtx.resume();
      const analyser = audioCtx.createAnalyser();
      analyser.fftSize = 256;
      analyser.smoothingTimeConstant = 0.75;
      audioCtx.createMediaStreamSource(stream).connect(analyser);
      audioCtxRef.current = audioCtx;
      startMeter(analyser);
      const recorder = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) chunks.push(event.data);
      };
      recorder.onstop = () => {
        stopMeter();
        stream.getTracks().forEach((track) => track.stop());
        const blob = new Blob(chunks, { type: recorder.mimeType || "audio/webm" });
        if (blob.size < 1024) return; // an accidental tap: nothing to transcribe
        setTranscribing(true);
        blob.arrayBuffer()
          .then((buffer) => {
            const bytes = new Uint8Array(buffer);
            let binary = "";
            const step = 0x8000;
            for (let i = 0; i < bytes.length; i += step) {
              binary += String.fromCharCode(...bytes.subarray(i, i + step));
            }
            return chat.transcribe(btoa(binary), blob.type || "audio/webm");
          })
          .then((text) => {
            if (text) insertAtCaret(text + " ");
          })
          .catch((err) => setVoiceError(err instanceof Error ? err.message : "Transcription failed"))
          .finally(() => setTranscribing(false));
      };
      recorder.start();
      recorderRef.current = recorder;
      setRecording(true);
    } catch (err) {
      setVoiceError(err instanceof Error ? err.message : "Microphone unavailable");
    }
  };

  const stopDictation = () => {
    stopMeter();
    recorderRef.current?.stop();
    setRecording(false);
  };

  // startMeter runs the mirrored frequency-bar canvas for as long as the
  // recording lasts; the canvas only exists while `recording`, so frames
  // before it mounts are skipped.
  const startMeter = (analyser: AnalyserNode) => {
    const data = new Uint8Array(analyser.frequencyBinCount);
    let color = "";
    const frame = () => {
      meterRafRef.current = requestAnimationFrame(frame);
      const canvas = meterRef.current;
      if (!canvas) return;
      const g = canvas.getContext("2d");
      if (!g) return;
      analyser.getByteFrequencyData(data);
      const { width, height } = canvas;
      g.clearRect(0, 0, width, height);
      if (!color) color = getComputedStyle(canvas).color;
      g.fillStyle = color;
      const bars = 14;
      const barWidth = 2;
      const gap = 2;
      for (let i = 0; i < bars; i++) {
        // Voice energy sits in the low bins; the power curve spreads the
        // bars across it instead of lighting only the first few.
        const bin = Math.min(
          data.length - 1,
          Math.floor(Math.pow(i / bars, 1.6) * (data.length * 0.5)) + 2,
        );
        const level = data[bin] / 255;
        const barHeight = Math.max(2, level * height);
        g.fillRect(i * (barWidth + gap), (height - barHeight) / 2, barWidth, barHeight);
      }
    };
    cancelAnimationFrame(meterRafRef.current);
    meterRafRef.current = requestAnimationFrame(frame);
  };

  const stopMeter = () => {
    cancelAnimationFrame(meterRafRef.current);
    void audioCtxRef.current?.close().catch(() => {});
    audioCtxRef.current = null;
  };

  // Hard cleanup if the page unloads mid-recording.
  useEffect(() => () => stopMeter(), []);

  return (
    <div className="chat">
      <div className="chat-log" ref={logRef} onScroll={onLogScroll}>
        {state?.hasOlder ? (
          <button type="button" className="older" onClick={() => void loadOlder()} disabled={loadingOlder}>
            {loadingOlder ? "Loading…" : "Earlier messages"}
          </button>
        ) : null}
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
        {showPendingAssistant ? (
          <div className="turn assistant pending" role="status" aria-live="polite">
            <div className="work-live">Waiting<span className="dots" /></div>
            <div className="skeleton" />
            <div className="skeleton short" />
          </div>
        ) : null}
      </div>
      {showJump ? (
        <button type="button" className="jump" onClick={jumpToLatest}>
          New messages ↓
        </button>
      ) : null}
      {state?.interactions?.length ? (
        <div className="dock" role="alert">
          {state.interactions.map((interaction) => (
            <div className="interaction" key={interaction.id}>
              <div className="interaction-head">
                <span className="pill">
                  {interaction.kind === "permission" ? "Approval" : "Question"}
                </span>
                <span className="interaction-prompt">{interaction.prompt}</span>
              </div>
              {textPrompt?.id === interaction.id ? (
                <div className="interaction-answer">
                  <textarea
                    autoFocus
                    value={textAnswer}
                    placeholder={`Answering “${textPrompt.option}” — type your answer…`}
                    aria-label="Free-text answer"
                    onChange={(event) => setTextAnswer(event.target.value)}
                    onKeyDown={onKeyDownAnswerText}
                    onCompositionStart={() => (composingRef.current = true)}
                    onCompositionEnd={() => (composingRef.current = false)}
                  />
                  <div className="interaction-answer-row">
                    <button
                      type="button"
                      onClick={() => {
                        setTextPrompt(null);
                        setTextAnswer("");
                      }}
                    >
                      Back
                    </button>
                    <span className="interaction-answer-hint">Enter submits · Esc goes back</span>
                    <button
                      type="button"
                      className="answer-send"
                      disabled={textAnswer.trim() === "" || answering}
                      onClick={() => void submitTextAnswer()}
                    >
                      {answering ? "Sending…" : "Send answer"}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="interaction-options">
                  {interaction.options.map((option, index) => (
                    <button
                      key={option}
                      type="button"
                      onClick={() => void answerInteraction(interaction.id, option)}
                    >
                      <span className="keycap">{index + 1}</span>
                      {option}
                    </button>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
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
        {completions.length > 0 ? (
          <div className="completions">
            {completions.map((name, index) => (
              <button
                key={name}
                type="button"
                className={index === completionPick ? "pick" : ""}
                onMouseDown={(event) => {
                  event.preventDefault(); // keep the textarea focus
                  acceptCompletion(index);
                }}
              >
                {name}
              </button>
            ))}
          </div>
        ) : null}
        <div className="composer-row">
          <button
            type="button"
            className="attach"
            title="Attach a file (inserts an @path mention)"
            onClick={() => void attach()}
          >
            +
          </button>
          <button
            type="button"
            className={`attach mic${recording ? " recording" : ""}`}
            title={recording ? "Stop and transcribe" : "Dictate"}
            disabled={transcribing}
            onClick={() => (recording ? stopDictation() : void startDictation())}
          >
            {recording ? "■" : "Mic"}
          </button>
          {recording ? <canvas ref={meterRef} className="mic-meter" width={44} height={28} aria-hidden /> : null}
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
          {working && draft.trim() !== "" ? (
            <button
              type="button"
              className="interrupt"
              title="Interrupt the agent and send this now"
              disabled={sending || interrupting}
              onClick={() => void interruptSend()}
            >
              {interrupting ? "Interrupting…" : "⚡ Interrupt & send"}
            </button>
          ) : null}
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
        <div className="composer-status">
          <span className="composer-note-text">
            {voiceError || (state?.interactions?.length
              ? "Waiting for your answer above — no need to touch the terminal."
              : disposition === "terminal"
                ? "Blocked — use the terminal."
                : working
                  ? "Working — Queue adds a follow-up; ⚡ Interrupt & send cuts in."
                  : transcribing
                    ? "Transcribing…"
                    : recording
                      ? "Listening… click ■ to transcribe."
                      : "")}
          </span>
          <span className="composer-facts">
            {state?.contextTokens ? <ContextRing used={state.contextTokens} /> : null}
            {state?.model ? <span className="model-pill">{state.model}</span> : null}
          </span>
        </div>
      </div>
    </div>
  );
}

function turnKey(turn: ChatBridgeState["turns"][number], index: number): string {
  return `${index}:${turn.user ? "u" : "a"}`;
}

// isFreeTextOption matches the AskUserQuestion affordances that expect a
// typed answer instead of a plain selection: the TUI's own "Type something."
// / "Chat about this" rows and the auto-appended "Other". Picking one
// without typing is exactly what produced "no answer provided".
function isFreeTextOption(option: string): boolean {
  return /^(type something|chat about|other)\b/i.test(option.trim());
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

// livePhaseLabel names the in-flight turn's stage the way the agent's own
// TUI does: nothing landed yet → Waiting; only thinking → Thinking; narration
// or tool runs → Working (the streaming caret takes over from there).
function livePhaseLabel(turn: ChatBridgeState["turns"][number]): string {
  if (turn.narration || (turn.tools?.length ?? 0) > 0) return "Working";
  if (turn.thinking) return "Thinking";
  return "Waiting";
}

function TurnRow({ turn }: { turn: ChatBridgeState["turns"][number] }) {
  if (turn.user) {
    return (
      <div className="turn user">
        <div className="bubble">
          {turn.text}
          <CopyButtons text={turn.text ?? ""} className="bubble-copy" />
        </div>
      </div>
    );
  }
  const failedCount = turn.tools?.filter((tool) => tool.isError).length ?? 0;
  return (
    <div className={`turn assistant${turn.live ? " live" : ""}`}>
      <div className="meta">
        <span className="who">ASSISTANT</span>
        {turn.failed ? <span className="pill error">Stopped</span> : null}
        {turn.tsMs ? (
          <span className="ts">{formatTime(turn.tsMs)}</span>
        ) : null}
        <span className="spacer" />
        {turn.narration ? (
          <CopyButtons text={turn.narration} markdown={turn.narration} />
        ) : null}
      </div>
      {turn.live ? <div className="work-live">{livePhaseLabel(turn)}<span className="dots" /></div> : null}
      {turn.narration ? <Markdown text={turn.narration} /> : null}
      {turn.thinking ? (
        <details className="block">
          <summary>Thinking</summary>
          <div className="block-body muted">{turn.thinking}</div>
        </details>
      ) : null}
      {turn.tools?.length ? (
        <details className="block" open={turn.live || turn.tools.length === 1}>
          <summary>
            <WorkSummary turn={turn} failedCount={failedCount} />
          </summary>
          <div className="block-body">
            {turn.tools.map((tool, index) => (
              <details
                key={index}
                className={tool.isError ? "tool error" : "tool"}
              >
                <summary>
                  {toolVerb(tool.name)} {tool.name}
                </summary>
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

// WorkSummary is the folded work-block header: the live "Working…" state
// settles into a per-category step count with failures carried out.
function WorkSummary({
  turn,
  failedCount,
}: {
  turn: ChatBridgeState["turns"][number];
  failedCount: number;
}) {
  const counts = new Map<string, number>();
  for (const tool of turn.tools ?? []) {
    const category = toolCategory(tool.name);
    counts.set(category, (counts.get(category) ?? 0) + 1);
  }
  const order = ["edit", "command", "search", "read", "web", "agent", "plan"];
  const parts: string[] = [];
  for (const category of order) {
    const count = counts.get(category);
    if (count) {
      parts.push(`${count} ${category}${count > 1 ? "s" : ""}`);
    }
  }
  const uncategorized = counts.get("step") ?? 0;
  if (uncategorized) {
    parts.push(`${uncategorized} step${uncategorized > 1 ? "s" : ""}`);
  }
  const total = turn.tools?.length ?? 0;
  const label = parts.length > 0 ? parts.join(" · ") : `${total} step${total === 1 ? "" : "s"}`;
  return (
    <span className="work-summary">
      {label}
      {failedCount > 0 ? <span className="pill error">{failedCount} failed</span> : null}
    </span>
  );
}

// Category vocabulary over the providers' tool names (reference:
// herdr-web-ui toolVerbs/workBlocks).
function toolCategory(name: string): string {
  if (/edit|write|patch|notebook|apply/i.test(name)) return "edit";
  if (/bash|command|exec|shell|terminal|run/i.test(name)) return "command";
  if (/grep|glob|search|find|list/i.test(name)) return "search";
  if (/read|view|cat\b/i.test(name)) return "read";
  if (/web|fetch|http|browse/i.test(name)) return "web";
  if (/task|agent|dispatch|spawn/i.test(name)) return "agent";
  if (/todo|plan/i.test(name)) return "plan";
  return "step";
}

function toolVerb(name: string): string {
  const category = toolCategory(name);
  switch (category) {
    case "edit": return "Edited with";
    case "command": return "Ran";
    case "search": return "Searched with";
    case "read": return "Read with";
    case "web": return "Fetched with";
    case "agent": return "Delegated to";
    case "plan": return "Planned with";
    default: return "Used";
  }
}

// ContextRing shows the provider-recorded context fill against the
// standard window, switching to the long window past it (web-ui's
// heuristic; hidden when the transcript records no usage).
function ContextRing({ used }: { used: number }) {
  const window = used > 200000 ? 1000000 : 200000;
  const fraction = Math.min(1, used / window);
  const low = fraction > 0.8;
  const radius = 7;
  const circumference = 2 * Math.PI * radius;
  return (
    <span
      className="ring-wrap"
      title={`Context ~${Math.round(used / 1000)}k of ${window / 1000}k tokens`}
    >
      <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden>
        <circle cx="9" cy="9" r={radius} fill="none" stroke="var(--border-subtle)" strokeWidth="2.5" />
        <circle
          cx="9" cy="9" r={radius} fill="none"
          stroke={low ? "var(--danger)" : "var(--working)"}
          strokeWidth="2.5"
          strokeDasharray={`${fraction * circumference} ${circumference}`}
          strokeLinecap="round"
          transform="rotate(-90 9 9)"
        />
      </svg>
    </span>
  );
}

function formatTime(tsMs: number): string {
  return new Date(tsMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// CopyButtons is the hover meta copy pair: Markdown source and stripped
// plain text (reference: herdr-web-ui's dual MD/TXT copy).
function CopyButtons({
  text,
  markdown,
  className,
}: {
  text: string;
  markdown?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState("");
  const copy = (value: string, kind: string) => {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(kind);
      window.setTimeout(() => setCopied(""), 1500);
    });
  };
  if (text === "") return null;
  return (
    <span className={`copybtns${className ? ` ${className}` : ""}`}>
      {markdown !== undefined ? (
        <button type="button" onClick={() => copy(markdown, "md")}>
          {copied === "md" ? "Copied" : "MD"}
        </button>
      ) : null}
      <button type="button" onClick={() => copy(plainText(text), "txt")}>
        {copied === "txt" ? "Copied" : "TXT"}
      </button>
    </span>
  );
}

// plainText strips common markdown syntax for the TXT copy variant.
function plainText(markdown: string): string {
  return markdown
    .replace(/```[^\n]*\n?/g, "")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_]{1,3}([^*_]+)[*_]{1,3}/g, "$1")
    .replace(/^\s*[-*+]\s+/gm, "• ");
}
