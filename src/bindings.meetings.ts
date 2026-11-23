import { invoke } from "@tauri-apps/api/core";

export type Result<T, E = string> =
  { status: "ok"; data: T } | { status: "error"; error: E };

export type MeetingChannel = "me" | "others";
export type MeetingState =
  "idle" | "consent_open" | "recording" | "paused" | "stopping";
export type MeetingWarning = "system_audio_silent" | "mic_fell_back_to_default";

export type SystemAudioFailureCause = "permission" | "device";

export type MeetingRefusal =
  | "already_recording"
  | "meetings_off"
  | { no_long_audio_model: { model_id: string } }
  | "system_audio_permission_denied"
  | "system_audio_usage_string_missing"
  | "system_audio_unavailable"
  | {
      system_audio_tap_failed: {
        cause: SystemAudioFailureCause;
        call: string;
        code: string;
      };
    };

export interface MeetingOffer {
  bundleId: string;
  app: "zoom" | "teams" | "meet" | string;
  inCall: boolean;
  startsCapture: boolean;
}

export interface MeetingStatus {
  state: MeetingState;
  elapsed_secs: number;
  paused?: boolean;
  title?: string | null;
  attendees?: string[];
  meeting_id: number | null;
  mic_name: string | null;
  mic_only?: boolean;
  warnings: MeetingWarning[];
  shortcut_failed?: boolean;
}

export interface MeetingListItem {
  id: number;
  title: string;
  started_at: number;
  ended_at: number | null;
  notes_ready: boolean;
  attendees?: string[];
  notes_finishing?: boolean;
  system_audio_silent?: boolean;
}

export interface MeetingSegmentView {
  id?: number;
  meeting_id: number;
  channel: MeetingChannel;
  speaker_label?: string | null;
  start_ms: number;
  end_ms: number;
  text: string;
}

export type NotesStateKind = "idle" | "running" | "ready" | "failed";

export interface MeetingNoteLineView {
  kind:
    | "summary"
    | "decision"
    | "action"
    | "question"
    | "decisions"
    | "actions"
    | "questions"
    | string;
  text: string;
  owner: string | null;
  start_ms: number | null;
  from_notepad: boolean;
}

export interface LeftOutSegmentView {
  category: "small_talk" | "uncovered" | "not_covered";
  start_ms: number;
  end_ms?: number;
  speaker?: string;
  text?: string;
  topic?: string;
  reason?: string;
  segment_ids?: number[];
}

export interface MeetingNotesSnapshot {
  meeting_id: number;
  state: NotesStateKind;
  progress_label: string | null;
  progress_step?: number;
  progress_total?: number;
  elapsed_secs: number;
  estimate_secs?: number | null;
  time_to_notes_secs?: number | null;
  failure_reason?: string | null;
  notepad_text: string;
  lines: MeetingNoteLineView[];
  left_out?: LeftOutSegmentView[];
  oversized_count: number;
}

export interface MeetingRetentionState {
  delete_audio_after_notes: boolean;
  transcript_delete_after_days?: number | null;
}

export interface DeleteAllMeetingsResult {
  deleted_count: number;
  audio_left_count: number;
}

export interface MeetingTodoItem {
  id: string;
  text: string;
  meeting_id: number;
  meeting_title: string;
  meeting_date: string;
  start_ms: number;
  done: boolean;
  said_by_me?: boolean;
}

export type AgentReadKind = "notes" | "transcript" | "live" | "list";

export interface AgentReadAuditEntry {
  id: string;
  client: string;
  kind: AgentReadKind;
  target_title: string;
  ts: number;
}

export interface AgentAccessSnapshot {
  enabled: boolean;
  allow_live_meeting: boolean;
  allow_past_meetings: boolean;
  recent_reads: AgentReadAuditEntry[];
}

export type BlockedReason = "work";

export interface CalendarChoice {
  id: string;
  title: string;
  account: string;
  blocked: BlockedReason | null;
}

export interface UpcomingEvent {
  title: string;
  start: number;
  end: number;
  calendar_name: string;
  attendee_display_names: string[];
}

export interface MeetingSpeakerSlot {
  slot: string;
  custom_name: string;
  display_name?: string | null;
}

export interface SpanDeleteOutcome {
  removed_segments: number;
  removed_notes: number;
  wiped_audio_files: number;
}

export interface TranscriptMatchRange {
  start: number;
  end: number;
}

export interface TranscriptSearchHit {
  segment_id: number;
  start_ms: number;
  end_ms: number;
  channel: string;
  text: string;
  ranges: TranscriptMatchRange[];
}

export interface FolderMeetingItem {
  id: number;
  title: string;
  started_at: number;
  ended_at: number | null;
  notes_ready: boolean;
}

export interface MeetingFolderView {
  id: number;
  name: string;
  created_at: number;
  last_template_id: number | null;
  meetings: FolderMeetingItem[];
}

export interface MeetingTemplateView {
  id: number;
  name: string;
  body: string;
  created_at: number;
}

export interface AskTurnInput {
  question: string;
  answer: string;
}

export interface AskCitationV2 {
  meeting_id: number;
  meeting_title: string;
  date: string;
  at_ms: number;
  segment_id: number;
  label: string;
}

export interface AskMeetingsV2Result {
  answer: string;
  citations: AskCitationV2[];
}

export interface ImportFailureItem {
  name: string;
  reason: string;
}

export interface ImportBatchSummary {
  folder: string;
  imported_count: number;
  skipped_count: number;
  meeting_ids: number[];
  failed: ImportFailureItem[];
}

export interface ImportCountSummary {
  count: number;
  display_folder: string | null;
}

export interface TrashedMeetingRow {
  id: number;
  title: string;
  deleted_at: number;
}

async function callCmd<T, E = string>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<Result<T, E>> {
  try {
    const data = await invoke<T>(cmd, args);
    return { status: "ok", data };
  } catch (err) {
    return { status: "error", error: err as E };
  }
}

export const commands = {
  changeMeetingsEnabledSetting: (enabled: boolean) =>
    callCmd<null>("change_meetings_enabled_setting", { enabled }),
  currentMeetingOffer: () =>
    callCmd<MeetingOffer | null>("current_meeting_offer"),
  dismissMeetingOffer: (bundleId: string) =>
    callCmd<null>("dismiss_meeting_offer", { bundleId }),
  openMeetingConsent: () => callCmd<MeetingStatus>("open_meeting_consent"),
  confirmMeetingStart: () =>
    callCmd<MeetingStatus, MeetingRefusal>("confirm_meeting_start"),
  confirmMeetingStartMicOnly: () =>
    callCmd<MeetingStatus, MeetingRefusal>("confirm_meeting_start_mic_only"),
  openSystemAudioPrivacySettings: () =>
    callCmd<null>("open_system_audio_privacy_settings"),
  cancelMeetingConsent: () => callCmd<MeetingStatus>("cancel_meeting_consent"),
  stopMeeting: () => callCmd<MeetingStatus>("stop_meeting"),
  pauseMeeting: () => callCmd<MeetingStatus>("pause_meeting"),
  resumeMeeting: () => callCmd<MeetingStatus>("resume_meeting"),
  requestAppendMeetingConsent: (meetingId: number) =>
    callCmd<number>("request_append_meeting_consent", { meetingId }),
  confirmAppendMeetingAudio: (meetingId: number, token: number) =>
    callCmd<MeetingStatus>("confirm_append_meeting_audio", {
      meetingId,
      token,
    }),
  appendMeetingAudio: (meetingId: number) =>
    callCmd<MeetingStatus>("append_meeting_audio", { meetingId }),
  async meetingStatus(): Promise<MeetingStatus> {
    try {
      return await invoke<MeetingStatus>("meeting_status");
    } catch {
      return {
        state: "idle",
        elapsed_secs: 0,
        attendees: [],
        meeting_id: null,
        mic_name: null,
        warnings: [],
      };
    }
  },
  meetingSegments: (meetingId: number) =>
    callCmd<MeetingSegmentView[]>("meeting_segments", { meetingId }),
  meetingNotesSnapshot: (meetingId: number) =>
    callCmd<MeetingNotesSnapshot>("meeting_notes_snapshot", { meetingId }),
  saveMeetingNotepad: (meetingId: number, text: string) =>
    callCmd<null>("save_meeting_notepad", { meetingId, text }),
  retryMeetingNotes: (meetingId: number) =>
    callCmd<MeetingNotesSnapshot>("retry_meeting_notes", { meetingId }),
  renameMeeting: (meetingId: number, title: string) =>
    callCmd<string>("rename_meeting", { meetingId, title }),
  catchUpMeeting: (meetingId: number, minutes?: number) =>
    callCmd<MeetingNoteLineView[]>(
      "catch_up_meeting",
      minutes !== undefined ? { meetingId, minutes } : { meetingId },
    ),
  async meetingRetentionState(): Promise<MeetingRetentionState> {
    try {
      return await invoke<MeetingRetentionState>("meeting_retention_state");
    } catch {
      return {
        delete_audio_after_notes: true,
        transcript_delete_after_days: null,
      };
    }
  },
  changeDeleteAudioAfterNotesSetting: (enabled: boolean) =>
    callCmd<MeetingRetentionState>("change_delete_audio_after_notes_setting", {
      enabled,
    }),
  changeTranscriptDeleteDaysSetting: (days: number | null) =>
    callCmd<MeetingRetentionState>("change_transcript_delete_days_setting", {
      days,
    }),
  requestDeleteAllMeetings: () =>
    callCmd<string>("request_delete_all_meetings"),
  deleteAllMeetings: (confirmationToken?: string | null) =>
    callCmd<DeleteAllMeetingsResult>(
      "delete_all_meetings",
      confirmationToken !== undefined
        ? { confirmationToken: confirmationToken ?? null }
        : undefined,
    ),
  restoreTrashedMeeting: (meetingId: number) =>
    callCmd<null>("restore_trashed_meeting", { meetingId }),
  deleteTranscriptSpan: (meetingId: number, startMs: number, endMs: number) =>
    callCmd<null>("delete_transcript_span", { meetingId, startMs, endMs }),
  importMeetingNotes: (paths: string[], folder: string) =>
    callCmd<number>("import_meeting_notes", { paths, folder }),
  askMeetings: (question: string, scope: string, meetingId?: number | null) =>
    callCmd<{
      answer: string;
      citations: Array<{ label: string; start_ms: number }>;
    }>("ask_meetings", { question, scope, meetingId: meetingId ?? null }),
  listMeetingTodos: () => callCmd<MeetingTodoItem[]>("list_meeting_todos"),
  completeMeetingTodo: (todoId: string, done: boolean) =>
    callCmd<null>("complete_meeting_todo", { todoId, done }),
  agentAccessState: () => callCmd<AgentAccessSnapshot>("agent_access_state"),
  mcpCommandLine: () => callCmd<string>("mcp_command_line"),
  setAgentAccessEnabled: (enabled: boolean) =>
    callCmd<AgentAccessSnapshot>("set_agent_access_enabled", { enabled }),
  setAgentAccessScope: (
    allowLiveMeeting: boolean,
    allowPastMeetings: boolean,
  ) =>
    callCmd<AgentAccessSnapshot>("set_agent_access_scope", {
      allowLiveMeeting,
      allowPastMeetings,
    }),
  getMeetingCustomWords: () => callCmd<string[]>("get_meeting_custom_words"),
  setMeetingCustomWords: (words: string[]) =>
    callCmd<string[]>("set_meeting_custom_words", { words }),
  listCalendarChoices: () => callCmd<CalendarChoice[]>("list_calendar_choices"),
  changeSelectedCalendarsSetting: (ids: string[]) =>
    callCmd<string[]>("change_selected_calendars_setting", { ids }),
  markCalendarAsWorkSetting: (id: string) =>
    callCmd<string[]>("mark_calendar_as_work_setting", { id }),
  listUpcomingMeetings: () =>
    callCmd<UpcomingEvent[]>("list_upcoming_meetings"),
  keepUpcomingForNotes: (start: number, title: string, calendarName: string) =>
    callCmd<UpcomingEvent | null>("keep_upcoming_for_notes", {
      start,
      title,
      calendarName,
    }),
  closeUpcomingMeetings: () => callCmd<null>("close_upcoming_meetings"),
  listMeetingSpeakers: (meetingId: number) =>
    import.meta.env.VITE_SPEAKER_SPLIT === "1"
      ? callCmd<MeetingSpeakerSlot[]>("list_meeting_speakers", { meetingId })
      : Promise.resolve<Result<MeetingSpeakerSlot[]>>({
          status: "ok",
          data: [],
        }),
  renameMeetingSpeakerSlot: (
    meetingId: number,
    speakerLabel: string,
    customName: string,
  ) =>
    import.meta.env.VITE_SPEAKER_SPLIT === "1"
      ? callCmd<MeetingSpeakerSlot[]>("rename_meeting_speaker_slot", {
          meetingId,
          speakerLabel,
          customName,
        })
      : Promise.resolve<Result<MeetingSpeakerSlot[]>>({
          status: "ok",
          data: [],
        }),
  setMeetingOfferVisible: (visible: boolean) =>
    callCmd<null>("set_meeting_offer_visible", { visible }),
  changeNotesMarkdownFolderSetting: (enabled: boolean, folder: string | null) =>
    callCmd<MeetingRetentionState>("change_notes_markdown_folder_setting", {
      enabled,
      folder,
    }),
  trashMeeting: (id: number) => callCmd<null>("trash_meeting", { id }),
  deleteMeetingForever: (id: number) =>
    callCmd<boolean>("delete_meeting_forever", { id }),
  listTrashedMeetingsV2: () =>
    callCmd<TrashedMeetingRow[]>("list_trashed_meetings_v2"),
  restoreTrashedMeetingV2: (meetingId: number) =>
    callCmd<null>("restore_trashed_meeting_v2", { meetingId }),
  purgeExpiredTrashNow: () => callCmd<number>("purge_expired_trash_now"),
  deleteTranscriptSegments: (meetingId: number, segmentIds: number[]) =>
    callCmd<SpanDeleteOutcome>("delete_transcript_segments", {
      meetingId,
      segmentIds,
    }),
  deleteTranscriptSpanSecure: (
    meetingId: number,
    startMs: number,
    endMs: number,
  ) =>
    callCmd<SpanDeleteOutcome>("delete_transcript_span_secure", {
      meetingId,
      startMs,
      endMs,
    }),
  searchMeetingTranscript: (meetingId: number, query: string) =>
    callCmd<TranscriptSearchHit[]>("search_meeting_transcript", {
      meetingId,
      query,
    }),
  listMeetingFolders: () =>
    callCmd<MeetingFolderView[]>("list_meeting_folders"),
  createMeetingFolder: (name: string) =>
    callCmd<{
      id: number;
      name: string;
      created_at: number;
      last_template_id: number | null;
    }>("create_meeting_folder", { name }),
  renameMeetingFolder: (id: number, name: string) =>
    callCmd<null>("rename_meeting_folder", { id, name }),
  deleteMeetingFolder: (id: number) =>
    callCmd<null>("delete_meeting_folder", { id }),
  setMeetingFolder: (meetingId: number, folderId: number | null) =>
    callCmd<null>("set_meeting_folder", { meetingId, folderId }),
  listMeetingTemplates: () =>
    callCmd<MeetingTemplateView[]>("list_meeting_templates"),
  createMeetingTemplate: (name: string, body: string) =>
    callCmd<MeetingTemplateView>("create_meeting_template", { name, body }),
  updateMeetingTemplate: (id: number, name: string, body: string) =>
    callCmd<null>("update_meeting_template", { id, name, body }),
  deleteMeetingTemplate: (id: number) =>
    callCmd<null>("delete_meeting_template", { id }),
  setFolderLastTemplate: (folderId: number, templateId: number | null) =>
    callCmd<null>("set_folder_last_template", { folderId, templateId }),
  askMeetingsV2: (params: {
    question: string;
    scope: string;
    meetingId?: number | null;
    folderName?: string | null;
    meetingIds?: number[] | null;
    history?: AskTurnInput[];
  }) =>
    callCmd<AskMeetingsV2Result>("ask_meetings_v2", {
      question: params.question,
      scope: params.scope,
      meetingId: params.meetingId ?? null,
      folderName: params.folderName ?? null,
      meetingIds: params.meetingIds ?? null,
      history: params.history ?? [],
    }),
  startAskByVoice: () => callCmd<null>("start_ask_by_voice"),
  cancelAskByVoice: () => callCmd<null>("cancel_ask_by_voice"),
  finishAskByVoice: () => callCmd<string>("finish_ask_by_voice"),
  countImportableNotes: (paths: string[]) =>
    callCmd<ImportCountSummary>("count_importable_notes", { paths }),
  importMeetingNotesBatch: (paths: string[], folder?: string | null) =>
    callCmd<ImportBatchSummary>("import_meeting_notes_batch", {
      paths,
      folder: folder ?? null,
    }),
};
