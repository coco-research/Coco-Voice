import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { LiveTranscriptSection } from "../src/components/meetings/LiveTranscriptSection";
import {
  AddAudioControl,
  ImportNotesPanel,
  MeetingTrashPanel,
  OlderNoteWhileRecordingBanner,
} from "../src/components/meetings/MeetingHistoryExtras";
import {
  detectSyncedFolderService,
  MeetingsFilesAndLanguageSection,
} from "../src/components/settings/MeetingsFilesAndLanguage";
import { MeetingsRetentionSection } from "../src/components/settings/MeetingsRetention";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("Group 3 (A3, A4, A5, A6, A10) screens", () => {
  test("Frame 9 retention enforces two-step Delete all, keeps audio deletion on by default, and offers optional transcript timer", () => {
    const step0 = wrap(
      <MeetingsRetentionSection
        deleteAudioAfterNotes={true}
        transcriptDeleteAfterDays={null}
        deleteAllStep={0}
        onToggleDeleteAudio={() => {}}
        onChangeTranscriptTimer={() => {}}
        onRequestDeleteAllStep1={() => {}}
        onAdvanceDeleteAllStep2={() => {}}
        onCancelDeleteAll={() => {}}
        onConfirmDeleteAll={() => {}}
      />,
    );
    expect(step0).toContain("Delete audio after notes are made");
    expect(step0).toContain("Keep transcripts on this Mac");
    expect(step0).toContain("On this Mac");
    expect(step0).toContain("Delete all");
    expect(step0).toContain("Off (keep until deleted)");
    expect(step0).not.toContain("Confirm delete all");

    const step1 = wrap(
      <MeetingsRetentionSection
        deleteAudioAfterNotes={true}
        transcriptDeleteAfterDays={null}
        deleteAllStep={1}
        onToggleDeleteAudio={() => {}}
        onChangeTranscriptTimer={() => {}}
        onRequestDeleteAllStep1={() => {}}
        onAdvanceDeleteAllStep2={() => {}}
        onCancelDeleteAll={() => {}}
        onConfirmDeleteAll={() => {}}
      />,
    );
    expect(step1).toContain(
      "Delete every meeting, transcript, note, and recording?",
    );
    expect(step1).toContain("Continue");
    expect(step1).not.toContain("Confirm delete all");

    const step2 = wrap(
      <MeetingsRetentionSection
        deleteAudioAfterNotes={true}
        transcriptDeleteAfterDays={30}
        deleteAllStep={2}
        onToggleDeleteAudio={() => {}}
        onChangeTranscriptTimer={() => {}}
        onRequestDeleteAllStep1={() => {}}
        onAdvanceDeleteAllStep2={() => {}}
        onCancelDeleteAll={() => {}}
        onConfirmDeleteAll={() => {}}
      />,
    );
    expect(step2).toContain("Final check:");
    expect(step2).toContain("Confirm delete all");
  });

  test("Frame 12 Notes as Markdown files is off by default, warns only on synced cloud folders, and renders A5 Meeting language", () => {
    expect(detectSyncedFolderService("~/Documents/Meetings")).toBeNull();
    expect(detectSyncedFolderService("/Users/alex/Dropbox/Notes")).toBe(
      "Dropbox",
    );
    expect(
      detectSyncedFolderService(
        "/Users/alex/Library/Mobile Documents/com~apple~CloudDocs/Notes",
      ),
    ).toBe("iCloud Drive");

    const localHtml = wrap(
      <MeetingsFilesAndLanguageSection
        saveMarkdownEnabled={false}
        folderPath="~/Documents/Meetings"
        meetingLanguage="auto"
        perMeetingOverride={false}
        onToggleSaveMarkdown={() => {}}
        onFolderChange={() => {}}
        onLanguageChange={() => {}}
        onTogglePerMeetingOverride={() => {}}
      />,
    );
    expect(localHtml).toContain("Save notes as Markdown files");
    expect(localHtml).toContain("Auto-detect");
    expect(localHtml).not.toContain("synced-folder-warning");

    const dropboxHtml = wrap(
      <MeetingsFilesAndLanguageSection
        saveMarkdownEnabled={true}
        folderPath="/Users/alex/Dropbox/Meetings"
        meetingLanguage="fr"
        perMeetingOverride={true}
        onToggleSaveMarkdown={() => {}}
        onFolderChange={() => {}}
        onLanguageChange={() => {}}
        onTogglePerMeetingOverride={() => {}}
      />,
    );
    expect(dropboxHtml).toContain("This folder syncs with Dropbox.");
    expect(dropboxHtml).toContain("Override for this meeting");
  });

  test("renders A4 Pause/Resume & Add audio, A6 Trash/span delete/search/older-note live banner, and A10 Import notes", () => {
    const pausedHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "paused",
          elapsed_secs: 130,
          meeting_id: 21,
          mic_name: null,
          warnings: [],
        }}
        segments={[
          {
            meeting_id: 21,
            channel: "me",
            start_ms: 10_000,
            end_ms: 14_000,
            text: "Budget line item for Q1.",
          },
          {
            meeting_id: 21,
            channel: "others",
            start_ms: 20_000,
            end_ms: 25_000,
            text: "Design tokens handoff.",
          },
        ]}
        pausedBadgeLabel={en.meetings.storage.pausedBadge}
        pausedHintLabel={en.meetings.storage.pausedHint}
        resumeLabel={en.meetings.storage.resume}
        onResume={() => {}}
        onStop={() => {}}
        searchQuery="budget"
        searchPlaceholder={en.meetings.storage.searchTranscriptPlaceholder}
        onSearchChange={() => {}}
        deleteSpanLabel={en.meetings.storage.deleteSpan}
        onDeleteSpan={() => {}}
      />,
    );
    expect(pausedHtml).toContain("Paused");
    expect(pausedHtml).toContain("02:10");
    expect(pausedHtml).toContain("Resume");
    expect(pausedHtml).toContain(
      "Paused: nothing is recorded. Still one meeting.",
    );
    expect(pausedHtml).toContain("Budget line item for Q1.");
    expect(pausedHtml).not.toContain("Design tokens handoff.");
    expect(pausedHtml).toContain("Delete this part");

    const addAudioHtml = wrap(
      <AddAudioControl onConfirmAppendAudio={() => {}} />,
    );
    expect(addAudioHtml).toContain("Add audio to this meeting");

    const bannerHtml = wrap(
      <OlderNoteWhileRecordingBanner
        liveTitle="Design sync"
        elapsedSecs={312}
        onBackToLive={() => {}}
      />,
    );
    expect(bannerHtml).toContain("Recording Design sync 05:12");
    expect(bannerHtml).toContain("Back to live notes");
    expect(bannerHtml).toContain(
      "Design sync keeps recording while you read this. The timer keeps running.",
    );

    const trashHtml = wrap(
      <MeetingTrashPanel
        items={[{ id: 9, title: "Sprint retro" }]}
        onRestore={() => {}}
      />,
    );
    expect(trashHtml).toContain("Deleted today · kept 30 days");
    expect(trashHtml).toContain("Restore");

    const importHtml = wrap(<ImportNotesPanel onImport={() => {}} />);
    expect(importHtml).toContain(
      "Bring notes from another meeting app. Exported Markdown or text files; nothing is fetched online.",
    );
    expect(importHtml).toContain("Import into folder: Imported");
    expect(importHtml).toContain("Import 3 notes");
  });
});
