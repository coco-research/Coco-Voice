import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

const invokeCalls: Array<{ cmd: string; args: unknown }> = [];
let startShouldFailWith: string | null = null;
let finishReturnText = "what is due thursday";

mock.module("@tauri-apps/api/core", () => ({
  transformCallback: () => 0,
  Channel: class {},
  convertFileSrc: (s: string) => s,
  invoke: async (cmd: string, args?: unknown) => {
    invokeCalls.push({ cmd, args });
    if (cmd === "start_ask_by_voice") {
      if (startShouldFailWith) {
        throw startShouldFailWith;
      }
      return null;
    }
    if (cmd === "finish_ask_by_voice") {
      return finishReturnText;
    }
    if (cmd === "cancel_ask_by_voice") {
      return null;
    }
    if (cmd === "ask_meetings_v2") {
      return {
        answer: "Empty-state strings are due before Thursday.",
        citations: [
          {
            meeting_id: 101,
            meeting_title: "Product sync",
            date: "Tue 27 Oct",
            at_ms: 588_000,
            segment_id: 3,
            label: "Product sync · 09:48",
          },
        ],
      };
    }
    return null;
  },
}));

const { commands } = await import("../src/bindings.meetings");
const {
  AskMeetingsSection,
  buildAskMeetingsV2Params,
  createAskByVoiceController,
  resolveMicPermissionErrorKey,
} = await import("../src/components/meetings/AskMeetingsView");

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

const FORBIDDEN_HISTORY_OR_PASTE = [
  "paste_last_take",
  "paste_text",
  "save_history",
  "add_history_entry",
  "get_history_entries",
];

describe("W27 / AC-X44: Ask by voice frontend flow (start, stop->ask, Esc cancel, mic denied)", () => {
  test("1 & 2: clicking Ask by voice listens; clicking again fills box and calls ask_meetings_v2 with exact typed args and zero history/paste", async () => {
    invokeCalls.length = 0;
    startShouldFailWith = null;
    finishReturnText = "what is due thursday";

    let listening = false;
    let voiceErrorKey: string | null = null;
    let draft = "";

    const scopeOpts = {
      isSingle: true,
      scope: "this_meeting" as const,
      meetingId: 101,
      selectedFolder: "",
      pickedMeetingIds: [] as number[],
      recentTurns: [],
    };

    const controller = createAskByVoiceController({
      startAskByVoice: () => commands.startAskByVoice(),
      finishAskByVoice: () => commands.finishAskByVoice(),
      cancelAskByVoice: () => commands.cancelAskByVoice(),
      onListeningChange: (v) => {
        listening = v;
      },
      onVoiceErrorKeyChange: (k) => {
        voiceErrorKey = k;
      },
      onSpokenQuestion: async (spoken) => {
        draft = spoken;
        const params = buildAskMeetingsV2Params(spoken, scopeOpts);
        await commands.askMeetingsV2(params);
      },
      platformHint: "MacIntel",
    });

    // Step 1: Click Ask by voice while idle -> listening state
    await controller.toggle();
    expect(listening).toBe(true);
    expect(voiceErrorKey).toBeNull();

    const listeningHtml = wrap(
      <AskMeetingsSection
        singleMeetingMode={true}
        meetingTitle="Product sync"
        scope="this_meeting"
        turns={[]}
        voiceListening={listening}
        voiceError={voiceErrorKey ? i18n.t(voiceErrorKey) : null}
        draftValue={draft}
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(listeningHtml).toContain('aria-pressed="true"');
    expect(listeningHtml).toContain("Listening for your question...");

    // Step 2: Click again -> stops, puts spoken words in box, sends ask_meetings_v2 once
    await controller.toggle();
    expect(listening).toBe(false);
    expect(draft).toBe("what is due thursday");

    const afterStopHtml = wrap(
      <AskMeetingsSection
        singleMeetingMode={true}
        meetingTitle="Product sync"
        scope="this_meeting"
        turns={[]}
        voiceListening={listening}
        voiceError={voiceErrorKey ? i18n.t(voiceErrorKey) : null}
        draftValue={draft}
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(afterStopHtml).toContain("what is due thursday");
    expect(afterStopHtml).toContain('aria-pressed="false"');

    // Compare against typing the exact same question in the exact same scope
    const askCalls = invokeCalls.filter((c) => c.cmd === "ask_meetings_v2");
    expect(askCalls.length).toBe(1);
    const typedParams = buildAskMeetingsV2Params(
      "what is due thursday",
      scopeOpts,
    );
    expect(askCalls[0]?.args).toEqual(typedParams);

    // Zero history or paste commands were ever called
    for (const call of invokeCalls) {
      expect(FORBIDDEN_HISTORY_OR_PASTE.includes(call.cmd)).toBe(false);
    }
  });

  test("3: Escape while listening cancels recording, leaves box unchanged, and makes zero ask calls", async () => {
    invokeCalls.length = 0;
    startShouldFailWith = null;

    let listening = false;
    let draft = "keep existing text";

    const controller = createAskByVoiceController({
      startAskByVoice: () => commands.startAskByVoice(),
      finishAskByVoice: () => commands.finishAskByVoice(),
      cancelAskByVoice: () => commands.cancelAskByVoice(),
      onListeningChange: (v) => {
        listening = v;
      },
      onVoiceErrorKeyChange: () => {},
      onSpokenQuestion: async (spoken) => {
        draft = spoken;
        await commands.askMeetingsV2({
          question: spoken,
          scope: "all",
        });
      },
      platformHint: "MacIntel",
    });

    await controller.toggle();
    expect(listening).toBe(true);

    const cancelled = await controller.cancel();
    expect(cancelled).toBe(true);
    expect(listening).toBe(false);
    expect(draft).toBe("keep existing text");
    expect(invokeCalls.map((c) => c.cmd)).toEqual([
      "start_ask_by_voice",
      "cancel_ask_by_voice",
    ]);
    expect(invokeCalls.filter((c) => c.cmd === "ask_meetings_v2").length).toBe(
      0,
    );
  });

  test("4: mic permission denied keeps button idle, shows macOS micPermissionDenied under box, and makes zero ask calls", async () => {
    invokeCalls.length = 0;
    startShouldFailWith = "mic_permission_denied: Microphone permission denied";

    let listening = false;
    let voiceErrorKey: string | null = null;

    const controller = createAskByVoiceController({
      startAskByVoice: () => commands.startAskByVoice(),
      finishAskByVoice: () => commands.finishAskByVoice(),
      cancelAskByVoice: () => commands.cancelAskByVoice(),
      onListeningChange: (v) => {
        listening = v;
      },
      onVoiceErrorKeyChange: (k) => {
        voiceErrorKey = k;
      },
      onSpokenQuestion: async (spoken) => {
        await commands.askMeetingsV2({ question: spoken, scope: "all" });
      },
      platformHint: "MacIntel",
    });

    await controller.toggle();
    expect(listening).toBe(false);
    expect(voiceErrorKey).toBe("errors.micPermissionDenied.macos");
    expect(resolveMicPermissionErrorKey("MacIntel")).toBe(
      "errors.micPermissionDenied.macos",
    );

    const deniedHtml = wrap(
      <AskMeetingsSection
        singleMeetingMode={false}
        scope="all"
        turns={[]}
        voiceListening={listening}
        voiceError={voiceErrorKey ? i18n.t(voiceErrorKey) : null}
        draftValue=""
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(deniedHtml).toContain('aria-pressed="false"');
    expect(deniedHtml).toContain("Ask by voice");
    expect(deniedHtml).toContain('data-testid="ask-voice-error"');
    expect(deniedHtml).toContain(
      en.errors.micPermissionDenied.macos.replace(/&/g, "&amp;"),
    );
    expect(invokeCalls.filter((c) => c.cmd === "ask_meetings_v2").length).toBe(
      0,
    );
  });
});
