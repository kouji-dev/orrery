/**
 * `<App>` — the only thing that holds a session.
 *
 * It subscribes once, feeds every frame to a `SurfaceStore` from
 * `@orrery/client`, and hands components plain data. Interaction bubbles back
 * up here, because a client never writes state: a person's edit leaves as an
 * `intent` the kernel validates.
 *
 * The screen is §6.4's hybrid. Settled turns go into `<Static>`, which prints
 * them once into native scrollback and never redraws them, so selection and
 * scrolling stay the terminal's. Only the live turn, the consent bar, the
 * footer and the composer are redrawn.
 */

import { Box, Static, Text, useStdout } from "ink";
import { useCallback, useEffect, useRef, useState, type ReactElement } from "react";

import { SurfaceStore, type Frame } from "@orrery/client";

import { Composer } from "./composer.js";
import { ConsentBar } from "./consent.js";
import { focusOf, pendingPrompt, type Focus } from "./focus.js";
import { Footer, type Usage } from "./footer.js";
import { Turn } from "./turn.js";

/**
 * Everything this client can ask a kernel to do.
 *
 * `AguiSession` from `@orrery/client` satisfies it; so does a fixture-backed
 * fake, which is how every test here runs without a model or a socket.
 */
export interface Connection {
  /** The event stream. Ends when the kernel hangs up. */
  frames(): AsyncIterable<Frame>;
  /** Re-attach, replaying from `since`. */
  attach(session: string, since?: number): Promise<void>;
  /** Start a turn. */
  submit(text: string): Promise<unknown>;
  /** Stop one. */
  cancel(turn: string): Promise<void>;
  /** Answer a consent prompt. */
  answer(prompt: string, answer: string): Promise<void>;
  /** Send what a surface produced. */
  intent(surface: string, value: unknown): Promise<void>;
}

/** What `<App>` needs. */
export interface AppProps {
  connection: Connection;
  /** The session to re-attach to after a gap. */
  session?: string;
  /** Column budget. Defaults to the terminal's width. */
  width?: number;
  /** Called once the frame stream ends. Tests wait on this. */
  onDrained?: (store: SurfaceStore) => void;
}

/** The whole client. */
export function App({ connection, session, width, onDrained }: AppProps): ReactElement {
  const { stdout } = useStdout();
  const columns = width ?? stdout?.columns ?? 80;
  const storeRef = useRef<SurfaceStore>(null as unknown as SurfaceStore);
  if (storeRef.current === null) storeRef.current = new SurfaceStore();
  const store = storeRef.current;

  const [, bump] = useState(0);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        for await (const frame of connection.frames()) {
          if (!alive) return;
          const before = store.state().last_seq;
          const changes = store.apply(frame);
          for (const change of changes) {
            if (change.kind !== "gap-detected") continue;
            setNotice(`lost frames ${change.expected}…${change.got - 1}; re-attaching`);
            // `since` is the last seq this client actually saw, never the one
            // it missed: the kernel replays from there.
            if (session) void connection.attach(session, before ?? undefined);
          }
          if (frame.type === "RUN_FINISHED") {
            const result = (frame as { result?: unknown }).result as Usage | null | undefined;
            if (result && typeof result === "object") setUsage(result);
          }
          bump((n) => n + 1);
        }
      } finally {
        if (alive) onDrained?.(store);
      }
    })();
    return () => {
      alive = false;
    };
    // The subscription is made once, on purpose: a re-subscribe would replay.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const state = store.state();
  const settled = state.turns.filter((t) => t.settled);
  const live = state.live === null ? null : (store.turn(state.live) ?? null);
  const prompt = pendingPrompt(state);

  const onIntent = useCallback(
    (surface: string, value: unknown) => {
      void connection.intent(surface, value);
    },
    [connection],
  );

  const onAnswer = useCallback(
    (id: string, answer: string) => {
      void connection.answer(id, answer);
    },
    [connection],
  );

  const onSubmit = useCallback(
    (text: string) => {
      void connection.submit(text);
    },
    [connection],
  );

  const onCancel = useCallback(() => {
    if (live) void connection.cancel(live.id);
  }, [connection, live]);

  const focus: Focus = focusOf(prompt, live);

  return (
    <Box flexDirection="column">
      <Static items={settled}>
        {(turn) => <Turn key={turn.id} turn={turn} width={columns} />}
      </Static>
      {live ? (
        <Turn turn={live} width={columns} active={focus === "surface"} onIntent={onIntent} />
      ) : null}
      {notice ? <Text color="yellow">{notice}</Text> : null}
      {prompt ? (
        <ConsentBar prompt={prompt} width={columns} active={focus === "consent"} onAnswer={onAnswer} />
      ) : null}
      <Footer usage={usage} live={live?.id ?? null} gaps={state.gaps.length} width={columns} />
      <Composer active={focus === "composer"} onSubmit={onSubmit} onCancel={onCancel} busy={live !== null} />
    </Box>
  );
}
