import { invoke } from "@tauri-apps/api/core";

export type Result<T, E = string> =
  { status: "ok"; data: T } | { status: "error"; error: E };

export type MeetingChannel = "me" | "others";
export type MeetingState =
  "idle" | "consent_open" | "recording" | "paused" | "stopping";
export type MeetingWarning = "system_audio_silent" | "mic_fell_back_to_default";

export type MeetingRefusal =
  | "already_recording"
  | { no_long_audio_model: { model_id: string } }
  | "system_audio_permission_denied"
  | "system_audio_usage_string_missing"
  | "system_audio_unavailable";

export interface MeetingOffer {
  bundleId: string;
  app: "zoom" | "teams" | "meet" | string;
  inCall: boolean;
  startsCapture: boolean;
}

export interface MeetingStatus {
  state: MeetingState;
  elapsed_secs: number;
  meeting_id: number | null;
  mic_name: string | null;
  warnings: MeetingWarning[];
  shortcut_failed?: boolean;
}

export interface MeetingSegmentView {
  meeting_id: number;
  channel: MeetingChannel;
  speaker_label?: string | null;
  start_ms: number;
  end_ms: number;
  text: string;
}

export type NotesStateKind = "idle" | "running" | "ready" | "failed";

export interface MeetingNoteLineView {
  kind: "summary" | "decisions" | "actions" | "questions" | string;
  text: string;
  owner: string | null;
  start_ms: number | null;
  from_notepad: boolean;
}

export interface LeftOutSegmentView {
  category: "small_talk" | "uncovered";
  start_ms: number;
  speaker: string;
  text: string;
}

export interface MeetingNotesSnapshot {
  meeting_id: number;
  state: NotesStateKind;
  progress_label: string | null;
  progress_step?: number;
  progress_total?: number;
  elapsed_secs: number;
  time_to_notes_secs?: number | null;
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
}

export interface AgentReadAuditEntry {
  id: string;
  tool: string;
  agent_name: string;
  when_label: string;
}

export interface AgentAccessSnapshot {
  enabled: boolean;
  allow_live_meeting: boolean;
  allow_past_meetings: boolean;
  recent_reads: AgentReadAuditEntry[];
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
  cancelMeetingConsent: () => callCmd<MeetingStatus>("cancel_meeting_consent"),
  stopMeeting: () => callCmd<MeetingStatus>("stop_meeting"),
  pauseMeeting: () => callCmd<MeetingStatus>("pause_meeting"),
  resumeMeeting: () => callCmd<MeetingStatus>("resume_meeting"),
  appendMeetingAudio: (meetingId: number) =>
    callCmd<MeetingStatus>("append_meeting_audio", { meetingId }),
  async meetingStatus(): Promise<MeetingStatus> {
    try {
      return await invoke<MeetingStatus>("meeting_status");
    } catch {
      return {
        state: "idle",
        elapsed_secs: 0,
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
  catchUpMeeting: (meetingId: number) =>
    callCmd<MeetingNoteLineView[]>("catch_up_meeting", { meetingId }),
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
  deleteAllMeetings: () =>
    callCmd<DeleteAllMeetingsResult>("delete_all_meetings"),
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
};
