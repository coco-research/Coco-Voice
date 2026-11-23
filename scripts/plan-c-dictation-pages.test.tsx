import { describe, expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { useSettingsStore } from "../src/stores/settingsStore";

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));
mock.module("../src/i18n", () => ({
  default: i18next,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

import type { AppProfileWithCleanup } from "../src/components/settings/AppProfilesView";

const { AppProfilesSection, summarizeAppProfile } =
  await import("../src/components/settings/AppProfilesView");
const { NeverDictateCard, formatBlockedSiteRow } =
  await import("../src/components/settings/NeverDictateCard");
const {
  WordsAndCleanupV022,
  formatWhereCleanupRuns,
  normalizeCanonicalSavedBlock,
} =
  await import("../src/components/settings/post-processing/PostProcessingSettings");

const ROOT = join(import.meta.dir, "..");

async function makeI18n() {
  const inst = i18next.createInstance();
  await inst.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function withSettings<T>(partial: Record<string, unknown>, fn: () => T): T {
  const g = globalThis as { window?: unknown };
  const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  const prevLoading = useSettingsStore.getState().isLoading;
  const prevSettings = useSettingsStore.getState().settings;
  const nextState = {
    isLoading: false,
    settings: { ...(prevSettings ?? {}), ...partial } as never,
  };
  Object.assign(useSettingsStore.getInitialState(), nextState);
  useSettingsStore.setState(nextState);
  try {
    return fn();
  } finally {
    Object.assign(useSettingsStore.getInitialState(), {
      isLoading: prevLoading,
      settings: prevSettings,
    });
    useSettingsStore.setState({
      isLoading: prevLoading,
      settings: prevSettings,
    });
    if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
    else delete g.window;
  }
}

describe("Plan C P-b..P-f: Words & Clean-up, Remember a Fix, Saved Blocks, Per-App Profiles & Never Dictate", () => {
  test("AC-P9 & AC-P12: Per-app profiles render exact 4 seeded summaries, Direct tone, Clean-up/Repo selects, and no duplicate ReadFocusedTextCard", async () => {
    const i18n = await makeI18n();
    const t = (k: string, d?: string) => i18n.t(k, { defaultValue: d });

    const seeded: AppProfileWithCleanup[] = [
      {
        id: "seed-mail",
        name: "Mail",
        app_identifier: "com.apple.mail",
        tone: "formal",
        capitalization: "sentence",
        punctuation: "full",
        cleanup: "on",
        post_process_enabled: true,
      },
      {
        id: "seed-messages",
        name: "Messages",
        app_identifier: "com.apple.MobileSMS",
        tone: "casual",
        capitalization: "lowercase",
        punctuation: "drop_trailing_period",
        cleanup: "on",
        post_process_enabled: true,
      },
      {
        id: "seed-cursor",
        name: "Cursor",
        app_identifier: "com.todesktop.230313mzl4w4u92",
        tone: "code",
        capitalization: "default",
        punctuation: "drop_trailing_period",
        cleanup: "light",
        repo_words: true,
        post_process_enabled: false,
      },
      {
        id: "seed-terminal",
        name: "Terminal",
        app_identifier: "com.apple.Terminal",
        tone: "default",
        capitalization: "default",
        punctuation: "default",
        cleanup: "off",
        post_process_enabled: false,
      },
    ];

    expect(summarizeAppProfile(seeded[0]!, t)).toBe(
      "Full sentences · capitals and punctuation · clean-up on",
    );
    expect(summarizeAppProfile(seeded[1]!, t)).toBe(
      "Casual · no full stop at the end · clean-up on",
    );
    expect(summarizeAppProfile(seeded[2]!, t)).toBe(
      "Code names keep their casing · repo words on · clean-up light",
    );
    expect(summarizeAppProfile(seeded[3]!, t)).toBe(
      "Exactly what you said · clean-up off",
    );

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <AppProfilesSection
          profiles={seeded}
          blockedApps={["com.1password.1password"]}
          blockedSites={[
            {
              name: "Banking",
              browser_identifier: "Safari",
              domain: "bank.example",
            },
          ]}
          displayNames={{ "com.1password.1password": "1Password" }}
          onSaveProfiles={async () => {}}
          onSaveBlockedApps={async () => {}}
          onSaveBlockedSites={async () => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    expect(html).toContain(
      "Code names keep their casing · repo words on · clean-up light",
    );
    // AC-P16: displays friendly app name instead of raw bundle id.
    expect(html).toContain(">1Password</p>");
    expect(html).not.toContain(">com.1password.1password</p>");
    // D-P3: site row formatted as "<name> (<browser>, <domain>)".
    expect(html).toContain("Banking (Safari, bank.example)");
    // AC-P32: ReadFocusedTextCard is NOT duplicated on AppProfilesView.
    expect(html).not.toContain('data-testid="read-focused-text-card"');
  });

  test("AC-P16 & D-P3: NeverDictateCard formats website rules cleanly", async () => {
    const i18n = await makeI18n();
    expect(
      formatBlockedSiteRow({
        name: "Banking",
        browser_identifier: "Safari",
        domain: "bank.example",
      }),
    ).toBe("Banking (Safari, bank.example)");

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <NeverDictateCard
          blockedApps={["com.1password.1password"]}
          displayNames={{ "com.1password.1password": "1Password" }}
          blockedSites={[
            {
              name: "Banking",
              browser_identifier: "Safari",
              domain: "bank.example",
            },
          ]}
          onSaveBlockedApps={async () => {}}
          onSaveBlockedSites={async () => {}}
        />
      </I18nextProvider>,
    );
    expect(html).toContain(">1Password</p>");
    expect(html).toContain("Banking (Safari, bank.example)");
  });

  test("AC-P19, AC-P21, AC-P28, AC-P30: Words & Clean-up shows real provider/model, real fix offer only when waiting, canonical saved blocks, and no Remove button on block rows", async () => {
    const i18n = await makeI18n();
    const t = i18n.t.bind(i18n);

    expect(
      formatWhereCleanupRuns(
        { post_process_provider_id: "apple_intelligence" },
        t,
      ),
    ).toBe("On this Mac (Apple Intelligence)");
    expect(
      formatWhereCleanupRuns(
        {
          post_process_provider_id: "local_llm",
          post_process_models: {
            local_llm: "Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf",
          },
        },
        t,
      ),
    ).toBe("On this Mac (Qwen3-4B)");
    expect(
      formatWhereCleanupRuns(
        {
          post_process_provider_id: "openai",
          post_process_providers: [{ id: "openai", label: "OpenAI" }],
          post_process_models: { openai: "gpt-4o-mini" },
        },
        t,
      ),
    ).toBe("OpenAI (gpt-4o-mini)");

    // Canonical saved block has ONLY {id, cue, content} (zero stray trigger/expansion/name).
    const canonical = normalizeCanonicalSavedBlock(
      "  my address ",
      " 221 Example St ",
      "b-1",
    );
    expect(Object.keys(canonical).sort()).toEqual(["content", "cue", "id"]);
    expect(canonical).toEqual({
      id: "b-1",
      cue: "my address",
      content: "221 Example St",
    });

    // Without waiting offer: no remember-fix-card and zero hardcoded demo text.
    const noOfferHtml = withSettings(
      {
        post_process_enabled: true,
        post_process_provider_id: "apple_intelligence",
        custom_words: ["Coco Voice"],
        saved_blocks: [
          {
            id: "b-1",
            cue: "my address",
            content: "221 Example Street, Apt 4",
          },
        ],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <WordsAndCleanupV022 />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );
    expect(noOfferHtml).not.toContain('data-testid="remember-fix-card"');
    expect(noOfferHtml).not.toContain("cocoa voice");
    expect(noOfferHtml).toContain("On this Mac (Apple Intelligence)");
    expect(noOfferHtml).toContain('aria-label="Remove Coco Voice"');
    // D-P6: Remove lives inside Edit form only, NOT on the saved-block list row.
    const rowSlice = noOfferHtml.slice(
      noOfferHtml.indexOf('data-testid="saved-block-row"'),
      noOfferHtml.indexOf('data-testid="words-corrections-section"'),
    );
    expect(rowSlice).toContain(">Edit<");
    expect(rowSlice).not.toContain(">Remove<");

    // With a real waiting fix offer: card renders at top with event's real words.
    const withOfferHtml = withSettings(
      {
        post_process_enabled: true,
        post_process_provider_id: "apple_intelligence",
        custom_words: [],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <WordsAndCleanupV022
              initialFixOffer={{
                id: "fix-1",
                old: "postgress",
                new: "Postgres",
                app: "Mail",
              }}
            />
          </I18nextProvider>,
        )
          .replaceAll("&#x27;", "'")
          .replaceAll("&quot;", '"'),
    );
    expect(withOfferHtml).toContain('data-testid="remember-fix-card"');
    expect(withOfferHtml).toContain(
      'You changed "postgress" to "Postgres" in Mail.',
    );
    expect(withOfferHtml).toContain('Always write "Postgres"?');
    expect(withOfferHtml).toContain("No words yet.");
  });

  test("S2: Words & clean-up word chips and Agent access Never · Not offered use body font (no font-mono)", async () => {
    const i18n = await makeI18n();
    const wordsHtml = withSettings(
      {
        post_process_enabled: true,
        post_process_provider_id: "apple_intelligence",
        custom_words: ["Coco Voice", "Parakeet", "Whisper"],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <WordsAndCleanupV022 />
          </I18nextProvider>,
        ),
    );
    for (const word of ["Coco Voice", "Parakeet", "Whisper"]) {
      expect(wordsHtml).toContain(`<span>${word}</span>`);
      expect(wordsHtml).not.toMatch(
        new RegExp(`font-mono[^>]*>\\s*<span>${word}</span>`),
      );
    }

    const agentSrc = readFileSync(
      join(ROOT, "src/components/meetings/AgentAccessView.tsx"),
      "utf8",
    );
    const neverIdx = agentSrc.indexOf('t("meetings.agent.audioNever")');
    expect(neverIdx).toBeGreaterThan(-1);
    const neverTag = agentSrc.slice(Math.max(0, neverIdx - 120), neverIdx);
    expect(neverTag).not.toContain("font-mono");
    // Technical CLI command stays monospace.
    expect(agentSrc).toContain(
      '<code className="font-mono text-[13px] font-semibold text-text">',
    );
  });

  test("Static security & architecture guards: zero objc2 imports outside meetings, zero as any setting writes, never_store_history stand-down, canonicalize + denylist, and surface style before saved blocks", () => {
    const wordsSrc = readFileSync(
      join(
        ROOT,
        "src/components/settings/post-processing/PostProcessingSettings.tsx",
      ),
      "utf8",
    );
    const wordsV022 = wordsSrc.slice(
      wordsSrc.indexOf("export const WordsAndCleanupV022"),
    );
    expect(wordsV022).not.toContain("iterative_correction_enabled");
    expect(wordsV022).not.toContain("as any");
    expect(wordsV022).toContain("change_offer_remember_fix_setting");
    expect(wordsV022).toContain("change_repo_words_setting");
    expect(wordsV022).toContain("change_filler_word_removal_setting");

    const fixOfferRs = readFileSync(
      join(ROOT, "src-tauri/src/fix_offer.rs"),
      "utf8",
    );
    expect(fixOfferRs).not.toContain("use objc2");
    expect(fixOfferRs).toContain("never_store_history");
    expect(fixOfferRs).toContain("CFRetain");
    expect(fixOfferRs).toContain("CFRelease");
    expect(fixOfferRs).toContain("is_secure_field_role");

    const repoWordsRs = readFileSync(
      join(ROOT, "src-tauri/src/repo_words.rs"),
      "utf8",
    );
    expect(repoWordsRs).not.toContain("use objc2");
    expect(repoWordsRs).toContain("fs::canonicalize");
    expect(repoWordsRs).toContain("symlink_metadata");

    const appProfilesRs = readFileSync(
      join(ROOT, "src-tauri/src/app_profiles.rs"),
      "utf8",
    );
    expect(appProfilesRs).not.toContain("use objc2");
    expect(appProfilesRs).toContain("MAX_PAGE_HOST_AX_DEPTH: usize = 8");
    expect(appProfilesRs).toContain("MAX_PAGE_HOST_AX_NODES: usize = 200");
    expect(appProfilesRs).toContain("merge_legacy_never_dictate_into_blocked");

    const actionsRs = readFileSync(
      join(ROOT, "src-tauri/src/actions.rs"),
      "utf8",
    );
    const prodActions = actionsRs.split("#[cfg(test)]")[0] ?? actionsRs;
    const styleIdx = prodActions.indexOf("apply_surface_style");
    const blocksIdx = prodActions.indexOf("expand_saved_blocks");
    expect(styleIdx).toBeGreaterThan(-1);
    expect(blocksIdx).toBeGreaterThan(styleIdx);
  });
});
