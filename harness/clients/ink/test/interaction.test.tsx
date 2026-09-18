/**
 * Task 5: answering, consenting and interrupting.
 *
 * All three go through `<App>`, because `<App>` is the only thing holding a
 * session — that is the discipline these tests exist to hold in place.
 */

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import { App } from "../src/app.js";
import { SurfaceNode } from "../src/surfaces/index.js";
import { FakeConnection, frames, settle, surfaceFrom } from "./fake.js";

const ENTER = "\r";
const DOWN = "[B";
const CTRL_C = "";

describe("answering", () => {
  it("question.answer_is_an_intent", async () => {
    // `question-surface` stops with the question still running and the turn
    // unsettled, which is exactly when a person answers one.
    const connection = new FakeConnection(frames("question-surface").slice(0, 2));
    const ui = render(<App connection={connection} session="sess-1" width={60} />);
    await settle(12);

    expect(ui.lastFrame()).toContain("Which crate?");
    ui.stdin.write(DOWN);
    await settle(2);
    ui.stdin.write(ENTER);
    await settle(2);

    // Not a state write: an intent, addressed to the surface, carrying the
    // choice's value rather than its label.
    expect(connection.of("intent").map((c) => c.args)).toEqual([["q-1", "kernel"]]);
    connection.close();
    ui.unmount();
  });

  it("sends the choice a person actually picked, not the default", async () => {
    const connection = new FakeConnection(frames("question-surface").slice(0, 2));
    const ui = render(<App connection={connection} session="sess-1" width={60} />);
    await settle(12);
    // The fixture's default is `proto`; the cursor starts there.
    ui.stdin.write(ENTER);
    await settle(2);
    expect(connection.of("intent")[0]!.args).toEqual(["q-1", "proto"]);
    connection.close();
    ui.unmount();
  });
});

describe("consent", () => {
  it("consent.is_distinct", async () => {
    const connection = new FakeConnection(frames("consent-prompt").slice(0, 8));
    const ui = render(<App connection={connection} session="sess-1" width={60} />);
    await settle(16);

    const frame = ui.lastFrame() ?? "";
    // Structurally distinct: a border, a vocabulary of its own, and a deadline.
    expect(frame).toContain("CONSENT");
    expect(frame).toContain("allow-once");
    expect(frame).toContain("deny-always");
    expect(frame).toMatch(/[╭─╮]/);

    // A question, by contrast, is none of those things.
    const question = render(
      <SurfaceNode node={surfaceFrom("question-surface", "q-1")} width={60} />,
    );
    const asked = question.lastFrame() ?? "";
    expect(asked).not.toContain("CONSENT");
    expect(asked).not.toContain("allow-once");
    expect(asked).not.toMatch(/[╭─╮]/);
    question.unmount();

    // And answering it goes down the consent path, not the intent path.
    ui.stdin.write("a");
    await settle(2);
    expect(connection.of("answer").map((c) => c.args)).toEqual([["prompt-1", "allow-once"]]);
    expect(connection.of("intent")).toHaveLength(0);
    connection.close();
    ui.unmount();
  });
});

describe("the composer", () => {
  it("composer.ctrl_c_cancels", async () => {
    // Mid-turn: the tool call is running.
    const connection = new FakeConnection(frames("tool-call").slice(0, 8));
    const ui = render(<App connection={connection} session="sess-1" width={60} />);
    await settle(16);

    ui.stdin.write(CTRL_C);
    await settle(4);

    expect(connection.of("cancel").map((c) => c.args)).toEqual([["turn-2"]]);
    // And the client is still here: ^C stops the turn, it does not throw the
    // transcript away.
    expect(ui.lastFrame()).toContain("builtin.read");
    connection.close();
    ui.unmount();
  });

  it("submits what was typed", async () => {
    const connection = new FakeConnection(frames("text-only"));
    const ui = render(<App connection={connection} session="sess-1" width={60} />);
    await settle(16);

    for (const ch of "hello") ui.stdin.write(ch);
    await settle(2);
    expect(ui.lastFrame()).toContain("hello");
    ui.stdin.write(ENTER);
    await settle(2);

    expect(connection.of("submit").map((c) => c.args)).toEqual([["hello"]]);
    connection.close();
    ui.unmount();
  });
});
