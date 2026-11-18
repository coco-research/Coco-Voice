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
} from "../src/components/meetings/AgentAccessView";
import {
  AskMeetingsSection,
  filterValidCitations,
  MeetingTodosPanel,
} from "../src/components/meetings/AskMeetingsView";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("Group 4 (A7, A8, A9) screens", () => {
  test("renders Frame 6 single-meeting Ask, Frame 13 Ask across meetings with cited moments, and Ask by voice mic button", () => {
    expect(
      filterValidCitations([
        { meetingTitle: "Product sync", start_ms: 41_000 },
        { meetingTitle: "Bad", start_ms: Number.NaN },
      ]).length,
    ).toBe(1);

    const singleHtml = wrap(
      <AskMeetingsSection
        singleMeetingMode={true}
        scope="this_meeting"
        turns={[
          {
            id: "1",
            question: "Who owns the copy update?",
            answer: "Dana will send the draft by Thursday.",
            citations: [{ start_ms: 588_000 }],
          },
        ]}
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(singleHtml).toContain("Ask this meeting");
    expect(singleHtml).toContain("This meeting only");
    expect(singleHtml).toContain("[09:48]");
    expect(singleHtml).toContain("Ask by voice");

    const acrossHtml = wrap(
      <AskMeetingsSection
        scope="all"
        voiceListening={true}
        turns={[
          {
            id: "2",
            question: "What did we agree on export?",
            answer: "One plain Markdown file per meeting.",
            citations: [
              {
                meetingTitle: "Product sync",
                meetingDate: "Tue",
                start_ms: 262_000,
              },
            ],
          },
        ]}
        onScopeChange={() => {}}
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(acrossHtml).toContain("Ask across meetings");
    expect(acrossHtml).toContain("All meetings on this Mac");
    expect(acrossHtml).toContain("[Product sync, Tue · 04:22]");
    expect(acrossHtml).toContain("Listening for your question...");
  });

  test("renders A8 Your to-dos across meetings with moment citation links and Done action", () => {
    const todosHtml = wrap(
      <MeetingTodosPanel
        items={[
          {
            id: "t1",
            text: "Send empty-state copy draft by Thursday",
            meeting_id: 2,
            meeting_title: "Product sync",
            meeting_date: "Tue",
            start_ms: 588_000,
            done: false,
          },
        ]}
      />,
    );
    expect(todosHtml).toContain("Your to-dos");
    expect(todosHtml).toContain("From your last 14 days of meetings");
    expect(todosHtml).toContain("[Product sync, Tue · 09:48]");
    expect(todosHtml).toContain("Done");
    expect(todosHtml).toContain(
      "Each to-do opens the moment you said it. Found on this Mac; updated after every meeting.",
    );
  });

  test("renders A9 Page 5 Agent access off by default with child switches disabled & greyed out when off, stdio-only command, and audit log", () => {
    expect(MCP_STDIO_COMMAND).toBe("coco-voice mcp");

    const defaultHtml = wrap(<AgentAccessView />);
    expect(defaultHtml).toContain("Off by default");
    expect(defaultHtml).toContain(
      "Nothing leaves this Mac through Coco Voice.",
    );
    expect(defaultHtml).toContain("opacity-50 pointer-events-none");
    expect(defaultHtml).toContain("Never · Not offered");
    expect(defaultHtml).toContain("coco-voice mcp");
    expect(defaultHtml).toContain("Recent reads");
    expect(defaultHtml).toContain("list_meetings");

    const onHtml = wrap(
      <AgentAccessSection
        enabled={true}
        allowLiveMeeting={true}
        allowPastMeetings={false}
        recentReads={[]}
        onToggleEnabled={() => {}}
        onToggleLiveMeeting={() => {}}
        onTogglePastMeetings={() => {}}
      />,
    );
    expect(onHtml).not.toContain("opacity-50 pointer-events-none");
  });
});
