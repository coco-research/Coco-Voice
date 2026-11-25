import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  AgentAskPillView,
  formatMeetingElapsed,
  HandsFreePillView,
  MeetingPillView,
  RecordingPillStateView,
  truncateAgentQuestion,
  truncatePillAppName,
} from "../src/overlay/RecordingPillStates";
import { PillStatesHarness } from "./pill-states-harness";

const ROOT = join(import.meta.dir, "..");

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{node}</I18nextProvider>,
  ).replaceAll("&#x27;", "'");

describe("Plan E Pill States, Meeting Pill, Hands-Free, Agent Ask & Security Guards", () => {
  test("AC-E3 & D-E1: renders every pill state with exact drawn copy, Sent fallback when app unknown, and Ctrl+V on non-macOS", () => {
    expect(wrap(<RecordingPillStateView kind="working" />)).toContain(
      "Working",
    );
    expect(
      wrap(<RecordingPillStateView kind="sent" targetApp="Mail" />),
    ).toContain("Sent to Mail");

    // D-E1: Unknown app renders "Sent" (not "Sent to Mail").
    const unknownSent = wrap(<RecordingPillStateView kind="sent" />);
    expect(unknownSent).toContain(">Sent<");
    expect(unknownSent).not.toContain("Mail");

    // Long app name truncated to 24 chars + ellipsis.
    expect(truncatePillAppName("VeryLongApplicationNameExceeding24")).toBe(
      "VeryLongApplicationNameE…",
    );

    expect(wrap(<RecordingPillStateView kind="clipboard" />)).toContain(
      "On the clipboard",
    );
    expect(
      wrap(<RecordingPillStateView kind="kept_press_paste" isMac={true} />),
    ).toContain("Words kept · press ⌘V");
    expect(
      wrap(<RecordingPillStateView kind="kept_press_paste" isMac={false} />),
    ).toContain("Words kept · press Ctrl+V");
  });

  test("AC-E4: Hands-free pill renders real shortcut key and Escape cancel instruction", () => {
    const fnHtml = wrap(<HandsFreePillView shortcutKey="fn" />);
    expect(fnHtml).toContain('role="status"');
    expect(fnHtml).toContain("Hands-free · tap fn to stop · Esc cancels");

    const optHtml = wrap(<HandsFreePillView shortcutKey="Option+Space" />);
    expect(optHtml).toContain(
      "Hands-free · tap Option+Space to stop · Esc cancels",
    );
  });

  test("AC-E5 & D-E3: Meeting pill formats mm:ss and h:mm:ss, renders Stop, disables while stopping, and shows waiting agent count", () => {
    expect(formatMeetingElapsed(768)).toBe("12:48");
    expect(formatMeetingElapsed(3909)).toBe("1:05:09");

    const activeHtml = wrap(
      <MeetingPillView elapsedSecs={768} onStop={() => {}} />,
    );
    expect(activeHtml).toContain('role="status"');
    expect(activeHtml).toContain("●");
    expect(activeHtml).toContain("Recording 12:48");
    expect(activeHtml).toContain(">Stop<");
    expect(activeHtml).not.toContain('disabled=""');

    const hourHtml = wrap(
      <MeetingPillView elapsedSecs={3909} onStop={() => {}} />,
    );
    expect(hourHtml).toContain("Recording 1:05:09");

    const stoppingHtml = wrap(
      <MeetingPillView
        elapsedSecs={3909}
        stopping={true}
        waitingAgentQuestions={2}
        onStop={() => {}}
      />,
    );
    expect(stoppingHtml).toContain("Finishing notes");
    expect(stoppingHtml).not.toContain("●");
    expect(stoppingHtml).toContain("? 2 waiting");
    expect(stoppingHtml).toContain('disabled=""');
  });

  test("AC-E9 & AC-E10 UI: Agent ask renders 34a prompt with 300-char cap, 34b answer sent, and 34c singular/plural queue counts", () => {
    const longQ = "q".repeat(350);
    const truncated = truncateAgentQuestion(longQ);
    expect(Array.from(truncated).length).toBe(301);
    expect(truncated.endsWith("…")).toBe(true);

    const html34a = wrap(
      <AgentAskPillView
        agentName="Claude Code"
        question="Which branch?"
        shortcutKey="fn"
      />,
    ).replaceAll("&quot;", '"');
    expect(html34a).toContain(
      'Claude Code asks: "Which branch?" · hold fn to answer',
    );
    expect(html34a).not.toContain("more question");

    const html34b = wrap(
      <AgentAskPillView
        agentName="Claude Code"
        question=""
        answerSentTo="Claude Code"
      />,
    );
    expect(html34b).toContain("Answer sent to Claude Code");

    const html34cOne = wrap(
      <AgentAskPillView
        agentName="Claude Code"
        question="Which branch?"
        queuedCount={1}
      />,
    );
    expect(html34cOne).toContain("1 more question waiting");

    const html34cMany = wrap(
      <AgentAskPillView
        agentName="Claude Code"
        question="Which branch?"
        queuedCount={3}
      />,
    );
    expect(html34cMany).toContain("3 more questions waiting");
  });

  test("Pill harness renders all scenes cleanly", () => {
    for (const scene of [
      "listening",
      "streaming",
      "working",
      "sent",
      "sent-unknown",
      "clipboard",
      "kept",
      "hands-free",
      "meeting",
      "meeting-waiting",
      "agent-ask",
      "agent-answered",
      "agent-queued",
      "all",
    ] as const) {
      const html = wrap(<PillStatesHarness scene={scene} />);
      expect(html.length).toBeGreaterThan(20);
    }
  });

  test("Security Guards: Unix socket 0600/0700 + symlink refusal + peer UID + zero TCP + debug-only fail_next_paste + build.rs stopRecordingMeeting", () => {
    const bridgeSrc = readFileSync(
      join(ROOT, "src-tauri/src/agent_ask_bridge.rs"),
      "utf8",
    );
    const bridgeProd = bridgeSrc.split("#[cfg(test)]")[0] ?? bridgeSrc;
    expect(bridgeProd).toContain("0o700");
    expect(bridgeProd).toContain("0o600");
    expect(bridgeProd).toContain("symlink_metadata");
    expect(bridgeProd).toContain("is_symlink()");
    expect(bridgeProd).toContain("getpeereid");
    expect(bridgeProd).toContain("SO_PEERCRED");
    expect(bridgeProd).toContain("geteuid");
    expect(bridgeProd).not.toContain("TcpListener");
    expect(bridgeProd).not.toContain("TcpStream");

    const actionsSrc = readFileSync(
      join(ROOT, "src-tauri/src/actions.rs"),
      "utf8",
    );
    const actionsProd = actionsSrc.split("#[cfg(test)]")[0] ?? actionsSrc;
    for (const sym of [
      "static FAIL_NEXT_PASTE",
      "pub fn debug_set_fail_next_paste",
      "fn take_fail_next_paste",
      "if take_fail_next_paste()",
    ]) {
      const idx = actionsProd.indexOf(sym);
      expect(idx).toBeGreaterThan(-1);
      const before = actionsProd.slice(Math.max(0, idx - 120), idx);
      expect(before).toContain("#[cfg(debug_assertions)]");
    }

    // Preserve C1 serialized policy-gate privacy guards in production actions.rs!
    expect(actionsProd).toContain("stage_take_wav_for_snapshot");
    expect(actionsProd).toContain("save_entry_with_meta_for_snapshot");
    expect(actionsProd).toContain("discard_staged_wav");
    expect(actionsProd).toContain("dictation_persistence_policy");

    // build.rs ensures stopRecordingMeeting exists in TrayStrings.
    const buildRs = readFileSync(join(ROOT, "src-tauri/build.rs"), "utf8");
    expect(buildRs).toContain("stopRecordingMeeting");
    expect(buildRs).toContain("stop_recording_meeting");
  });
});
