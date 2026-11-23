import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  AgentAccessSection,
  AgentAccessView,
  MCP_STDIO_COMMAND,
  applyAgentAccessMutation,
  formatAgentClientName,
  formatAgentReadLine,
  formatAgentReadWhen,
  resolveAndCopyMcpCommand,
  type AgentAccessStateSnapshot,
  type AgentReadAuditRow,
} from "../src/components/meetings/AgentAccessView";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{node}</I18nextProvider>,
  )
    .replaceAll("&#x27;", "'")
    .replaceAll("&quot;", '"');

describe("Plan C Step P-a · Agent access (AC-P4, AC-P5, AC-P6)", () => {
  test("AC-P4: formats all four read kinds, Today / Yesterday / older dates, and all three mapped agent names", () => {
    // Fixed reference local clock: Wed 28 Oct 2026 15:30 local time
    const now = new Date(2026, 9, 28, 15, 30, 0);
    const todayTs = Math.floor(new Date(2026, 9, 28, 10, 12, 0).getTime() / 1000);
    const yesterdayTs = Math.floor(
      new Date(2026, 9, 27, 18, 40, 0).getTime() / 1000,
    );
    const olderTs = Math.floor(new Date(2026, 9, 20, 9, 5, 0).getTime() / 1000);

    expect(formatAgentClientName("claude-code")).toBe("Claude Code");
    expect(formatAgentClientName("cursor")).toBe("Cursor");
    expect(formatAgentClientName("codex")).toBe("Codex");
    expect(formatAgentClientName("Custom MCP")).toBe("Custom MCP");

    expect(formatAgentReadWhen(todayTs, i18n.t, now.getTime(), "en")).toBe(
      "Today 10:12",
    );
    expect(formatAgentReadWhen(yesterdayTs, i18n.t, now.getTime(), "en")).toBe(
      "Yesterday 18:40",
    );
    expect(formatAgentReadWhen(olderTs, i18n.t, now.getTime(), "en")).toBe(
      "Tue 20 Oct 09:05",
    );

    const rows: AgentReadAuditRow[] = [
      {
        id: "r-1",
        client: "claude-code",
        kind: "notes",
        target_title: "Product sync",
        ts: todayTs,
      },
      {
        id: "r-2",
        client: "cursor",
        kind: "transcript",
        target_title: "Design review",
        ts: yesterdayTs,
      },
      {
        id: "r-3",
        client: "codex",
        kind: "live",
        target_title: "Morning standup",
        ts: todayTs,
      },
      {
        id: "r-4",
        client: "claude-code",
        kind: "list",
        target_title: "",
        ts: olderTs,
      },
    ];

    expect(formatAgentReadLine(rows[0], i18n.t, now.getTime(), "en")).toBe(
      'Claude Code read "Product sync" notes · Today 10:12',
    );
    expect(formatAgentReadLine(rows[1], i18n.t, now.getTime(), "en")).toBe(
      'Cursor read "Design review" transcript · Yesterday 18:40',
    );
    expect(formatAgentReadLine(rows[2], i18n.t, now.getTime(), "en")).toBe(
      'Codex read the live transcript of "Morning standup" · Today 10:12',
    );
    expect(formatAgentReadLine(rows[3], i18n.t, now.getTime(), "en")).toBe(
      "Claude Code listed your meetings · Tue 20 Oct 09:05",
    );

    const rendered = wrap(
      <AgentAccessSection
        enabled={true}
        allowLiveMeeting={true}
        allowPastMeetings={true}
        recentReads={rows}
        onToggleEnabled={() => {}}
        onToggleLiveMeeting={() => {}}
        onTogglePastMeetings={() => {}}
        nowMs={now.getTime()}
      />,
    );
    expect(rendered).toContain(
      'Claude Code read "Product sync" notes · Today 10:12',
    );
    expect(rendered).toContain(
      'Cursor read "Design review" transcript · Yesterday 18:40',
    );
    expect(rendered).toContain(
      'Codex read the live transcript of "Morning standup" · Today 10:12',
    );
    expect(rendered).toContain(
      "Claude Code listed your meetings · Tue 20 Oct 09:05",
    );
  });

  test("AC-P5: no sample reads before first IPC answer, child switches disabled+aria-disabled when off, no AgentsCanAskCard, snapshot-driven mutations with error rollback", async () => {
    const initialHtml = wrap(<AgentAccessView />);
    expect(initialHtml).toContain("No agent has read anything yet.");
    expect(initialHtml).not.toContain('data-testid="agent-recent-reads-list"');
    expect(initialHtml).not.toContain("14:22");
    expect(initialHtml).not.toContain("list_meetings");
    expect(initialHtml).not.toContain("Agents can ask");
    expect(initialHtml).toContain(
      'data-testid="agent-switch-live" aria-disabled="true"',
    );
    expect(initialHtml).toContain(
      'data-testid="agent-switch-past" aria-disabled="true"',
    );
    // Both child switch checkboxes carry the HTML disabled attribute when off.
    const disabledInputs = initialHtml.match(/<input[^>]*disabled=""/g) ?? [];
    expect(disabledInputs.length).toBeGreaterThanOrEqual(2);

    const prev: AgentAccessStateSnapshot = {
      enabled: false,
      allow_live_meeting: true,
      allow_past_meetings: true,
      recent_reads: [],
    };
    const backendTruth: AgentAccessStateSnapshot = {
      enabled: true,
      allow_live_meeting: false,
      allow_past_meetings: true,
      recent_reads: [],
    };
    // Mutation adopts the snapshot returned by the command, never a local guess.
    const okOutcome = await applyAgentAccessMutation(prev, async () => ({
      status: "ok",
      data: backendTruth,
    }));
    expect(okOutcome.failed).toBe(false);
    expect(okOutcome.snapshot).toEqual(backendTruth);

    // Failed save rolls back to previous confirmed snapshot and flags error.
    const errOutcome = await applyAgentAccessMutation(prev, async () => ({
      status: "error",
      error: "disk full",
    }));
    expect(errOutcome.failed).toBe(true);
    expect(errOutcome.snapshot).toEqual(prev);

    const errorHtml = wrap(
      <AgentAccessSection
        enabled={false}
        allowLiveMeeting={true}
        allowPastMeetings={true}
        recentReads={[]}
        onToggleEnabled={() => {}}
        onToggleLiveMeeting={() => {}}
        onTogglePastMeetings={() => {}}
        saveErrorSlot="master"
        saveErrorText="Couldn't save. Try again."
      />,
    );
    expect(errorHtml).toContain("Couldn't save. Try again.");
  });

  test("AC-P6: Copy puts quoted binary path from mcp_command_line on clipboard, shows Copied state, and falls back with muted hint when path unavailable", async () => {
    const copiedTexts: string[] = [];
    const fullCmd =
      "'/Applications/Coco Voice.app/Contents/MacOS/coco-voice' mcp";
    const fullResult = await resolveAndCopyMcpCommand(
      { mcpCommandLine: async () => ({ status: "ok", data: fullCmd }) },
      (text) => {
        copiedTexts.push(text);
      },
    );
    expect(fullResult).toEqual({ command: fullCmd, usedFallback: false });
    expect(copiedTexts[0]).toBe(fullCmd);

    const fallbackResult = await resolveAndCopyMcpCommand(
      {
        mcpCommandLine: async () => ({
          status: "ok",
          data: MCP_STDIO_COMMAND,
        }),
      },
      (text) => {
        copiedTexts.push(text);
      },
    );
    expect(fallbackResult).toEqual({
      command: MCP_STDIO_COMMAND,
      usedFallback: true,
    });
    expect(copiedTexts[1]).toBe(MCP_STDIO_COMMAND);

    const errorFallbackResult = await resolveAndCopyMcpCommand(
      {
        mcpCommandLine: async () => ({
          status: "error",
          error: "unavailable",
        }),
      },
      (text) => {
        copiedTexts.push(text);
      },
    );
    expect(errorFallbackResult.usedFallback).toBe(true);

    const copiedHtml = wrap(
      <AgentAccessSection
        enabled={true}
        allowLiveMeeting={true}
        allowPastMeetings={true}
        recentReads={[]}
        onToggleEnabled={() => {}}
        onToggleLiveMeeting={() => {}}
        onTogglePastMeetings={() => {}}
        copied={true}
        copyFallbackHint={true}
      />,
    );
    expect(copiedHtml).toContain(">Copied<");
    expect(copiedHtml).toContain("coco-voice mcp");
    expect(copiedHtml).toContain(
      "Use the full path to Coco Voice if your agent can't find it.",
    );
  });

  test("Part 5 timing budget: opens in < 200 ms with 100 reads and renders newest 5 rows", () => {
    const baseTs = Math.floor(Date.now() / 1000);
    const hundredReads: AgentReadAuditRow[] = Array.from(
      { length: 100 },
      (_, idx) => ({
        id: `r-${idx}`,
        client: idx % 3 === 0 ? "claude-code" : idx % 3 === 1 ? "cursor" : "codex",
        kind: idx % 2 === 0 ? "notes" : "transcript",
        target_title: `Meeting ${idx}`,
        ts: baseTs - idx * 60,
      }),
    );

    const t0 = performance.now();
    const html = wrap(
      <AgentAccessSection
        enabled={true}
        allowLiveMeeting={true}
        allowPastMeetings={true}
        recentReads={hundredReads}
        onToggleEnabled={() => {}}
        onToggleLiveMeeting={() => {}}
        onTogglePastMeetings={() => {}}
      />,
    );
    const elapsedMs = performance.now() - t0;

    expect(elapsedMs).toBeLessThan(200);
    expect(html).toContain("Meeting 0");
    expect(html).toContain("Meeting 4");
    expect(html).not.toContain("Meeting 5");
  });
});
