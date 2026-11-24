import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  buildTemplateBodyFromSections,
  FoldersAndTemplatesSection,
  FoldersAndTemplatesView,
  parseTemplateSectionsFromBody,
  sanitizeFlatFolderName,
} from "../src/components/meetings/FoldersAndTemplatesView";
import {
  AskMeetingsSection,
  AskMeetingsView,
  formatAskCitationBadge,
} from "../src/components/meetings/AskMeetingsView";
import {
  formatDeletedAgoLabel,
  ImportNotesPanel,
  MeetingTrashPanel,
} from "../src/components/meetings/MeetingHistoryExtras";

const ROOT = resolve(import.meta.dir, "..");
const TAURI_SRC = join(ROOT, "src-tauri", "src");

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("Plan B2 Part 1 (X-a, X-b, X-c, X-j)", () => {
  test("X-a & AC-X1/X2/X4: Trash row format, Delete now menu, retention selector, and Rust secure_delete/checkpoint", () => {
    const historyOps = readFileSync(
      join(TAURI_SRC, "meeting_history_ops.rs"),
      "utf8",
    );
    const meetingsStore = readFileSync(
      join(TAURI_SRC, "managers", "meetings.rs"),
      "utf8",
    );
    expect(historyOps).toContain("PRAGMA secure_delete = ON;");
    expect(historyOps).toContain("with_meeting_io_lock");
    expect(historyOps).toContain("checkpoint_and_vacuum_secure");
    expect(meetingsStore).toContain("PRAGMA secure_delete = ON;");
    expect(meetingsStore).toContain("PRAGMA wal_checkpoint(TRUNCATE);");
    expect(meetingsStore).toContain("CREATE TABLE meeting_left_out");
    expect(meetingsStore).toContain(
      "ALTER TABLE meetings ADD COLUMN template_id",
    );
    expect(meetingsStore).toContain(
      "ALTER TABLE meetings ADD COLUMN attendees",
    );
    expect(meetingsStore).toContain(
      "ALTER TABLE meeting_folders ADD COLUMN last_template_id",
    );

    const tFn = (k: string, opts?: Record<string, unknown>) =>
      String(i18n.t(k, opts));
    expect(formatDeletedAgoLabel(1_800_000_000, 1_800_000_100, tFn)).toContain(
      "Deleted today",
    );
    expect(
      formatDeletedAgoLabel(1_800_000_000 - 3 * 86_400, 1_800_000_000, tFn),
    ).toBe("Deleted 3 days ago · kept 30 days");

    const trashHtml = wrap(
      <MeetingTrashPanel
        items={[{ id: 7, title: "Sprint review", deletedAt: 1_800_000_000 }]}
        transcriptDeleteDays={30}
        onRestore={() => {}}
        onDeleteNow={() => {}}
        onChangeTranscriptDeleteDays={() => {}}
      />,
    );
    expect(trashHtml).toContain("Sprint review");
    expect(trashHtml).toContain("Restore");
    expect(trashHtml).toContain("More actions");
    expect(trashHtml).toContain("30 days");

    const listSrc = readFileSync(
      join(ROOT, "src", "components", "meetings", "MeetingsList.tsx"),
      "utf8",
    );
    expect(listSrc).toContain("Delete this part");
    expect(listSrc).not.toContain('"Delete span"');
  });

  test("X-b & AC-X5..X8: Folders & Templates run with zero sample state and flat 60-char sanitization", () => {
    expect(sanitizeFlatFolderName("A/B\\C   D")).toBe("A B C D");
    expect(sanitizeFlatFolderName("x".repeat(80)).length).toBe(60);
    expect(
      parseTemplateSectionsFromBody(
        buildTemplateBodyFromSections([
          "Their needs",
          "Decisions",
          "Follow-ups",
        ]),
      ),
    ).toEqual(["Their needs", "Decisions", "Follow-ups"]);

    // AC-X8 / AC-X39: Real view starts with zero hardcoded sample names.
    const emptyView = wrap(<FoldersAndTemplatesView />);
    expect(emptyView).not.toContain("Renewal review");
    expect(emptyView).not.toContain("Onboarding walkthrough");
    expect(emptyView).not.toContain("Product sync");

    const rendered = wrap(
      <FoldersAndTemplatesSection
        folders={[
          {
            id: "1",
            name: "Customers",
            open: true,
            meetings: [
              {
                id: 10,
                title: "Kickoff call",
                dateLabel: "Tue 10:00 · 30 min",
                notesReady: true,
              },
            ],
          },
        ]}
        templates={[
          {
            id: "2",
            name: "Customer call",
            sections: [
              "Their needs",
              "Decisions",
              "Action items",
              "Follow-ups",
            ],
          },
        ]}
        selectedTemplateId="2"
        onToggleFolder={() => {}}
        onAddFolder={() => {}}
        onRenameFolder={() => {}}
        onDeleteFolder={() => {}}
        onSelectTemplate={() => {}}
        onStartWithTemplate={() => {}}
      />,
    );
    expect(rendered).toContain("Customers · 1 meetings");
    expect(rendered).toContain("Customer call");
    expect(rendered).toContain("Selected");
  });

  test("X-c & AC-X9..X12: Ask starts empty with zero DEFAULT_TURNS, preserves Folder/Picked scopes, and Rust prompt strips titles", () => {
    const askRs = readFileSync(join(TAURI_SRC, "meeting_ask.rs"), "utf8");
    const indexRs = readFileSync(
      join(TAURI_SRC, "meeting_ask_index.rs"),
      "utf8",
    );
    const promptFnStart = askRs.indexOf("pub fn build_ask_prompt_with_history");
    const promptFnEnd = askRs.indexOf("fn format_citation_display");
    const promptFnBody = askRs.slice(promptFnStart, promptFnEnd);
    expect(promptFnBody).toContain("<transcript_data>");
    expect(promptFnBody).toContain("untrusted data");
    expect(promptFnBody).not.toContain("meeting_title");
    expect(promptFnBody).not.toContain("timestamp");
    expect(indexRs).toContain("meeting_folders");
    expect(indexRs).toContain("meeting_trash");
    expect(indexRs).toContain("PRAGMA secure_delete = ON;");

    // AC-X12: AskMeetingsView starts empty with zero hardcoded sample turns.
    const emptyAsk = wrap(<AskMeetingsView />);
    expect(emptyAsk).not.toContain("What did we decide on the export format?");
    expect(emptyAsk).not.toContain("Product sync");
    expect(emptyAsk).toContain('data-testid="ask-scope-chip"');

    // I21 / D-X1: Folder and Picked scopes preserved.
    const folderScopeHtml = wrap(
      <AskMeetingsSection
        scope="folder"
        turns={[]}
        availableFolders={["Customers", "Engineering"]}
        selectedFolder="Customers"
        onScopeChange={() => {}}
        onSelectFolder={() => {}}
        onAsk={() => {}}
        onToggleVoiceAsk={() => {}}
      />,
    );
    expect(folderScopeHtml).toContain("Customers");
    expect(folderScopeHtml).toContain("Engineering");

    expect(
      formatAskCitationBadge({
        meetingId: 1,
        meetingTitle: "Quarterly review",
        meetingDate: "Oct 21",
        start_ms: 724_000,
      }),
    ).toBe("[Quarterly review, Oct 21 · 12:04]");
    expect(formatAskCitationBadge({ start_ms: 435_000 })).toBe("[07:15]");
  });

  test("X-j & AC-X34/X35: Import panel has zero sample paths, shows batch results, and Rust enforces O_NOFOLLOW & caps", () => {
    const importRs = readFileSync(join(TAURI_SRC, "meeting_import.rs"), "utf8");
    expect(importRs).toContain("MAX_IMPORT_FILE_BYTES: u64 = 1_048_576");
    expect(importRs).toContain("MAX_IMPORT_FILES: usize = 500");
    expect(importRs).toContain("MAX_IMPORT_DEPTH: usize = 3");
    expect(importRs).toContain("O_NOFOLLOW");
    expect(importRs).toContain("symlink_metadata");
    expect(importRs).toContain('let status = "imported"');

    // AC-X35: Default ImportNotesPanel has 0 sample paths and disabled Import button.
    const emptyImport = wrap(<ImportNotesPanel onImport={() => {}} />);
    expect(emptyImport).not.toContain("weekly-sync.md");
    expect(emptyImport).toContain("Import 0 notes");

    const withResult = wrap(
      <ImportNotesPanel
        selectedPaths={["/tmp/a.md", "/tmp/b.md"]}
        importableCount={2}
        displayFolder="~/Exports"
        targetFolder="Imported"
        resultSummary={{
          folder: "Imported",
          imported_count: 1,
          skipped_count: 1,
          meeting_ids: [42],
          failed: [{ name: "huge.md", reason: "too large" }],
        }}
        onImport={() => {}}
      />,
    );
    expect(withResult).toContain("2 files found in ~/Exports");
    expect(withResult).toContain("Imported 1. Skipped 1 already imported.");
    expect(withResult).toContain("huge.md: too large");
  });
});
