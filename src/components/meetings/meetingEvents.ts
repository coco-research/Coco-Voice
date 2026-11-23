import type {
  MeetingNoteLineView,
  MeetingRefusal,
} from "../../bindings.meetings";

export type PendingStartRefusal = MeetingRefusal | string;

let pendingStartRefusal: PendingStartRefusal | null = null;
const startRefusalHandOffListeners = new Set<
  (refusal: PendingStartRefusal) => void
>();

/** Hand off a refused start from the app-wide consent dialog to RecordMeetingPage (one-shot). */
export function handOffMeetingStartRefusal(refusal: PendingStartRefusal): void {
  pendingStartRefusal = refusal;
  for (const listener of [...startRefusalHandOffListeners]) {
    listener(refusal);
  }
}

/** Read any pending refused start without clearing (for StrictMode-safe initial state). */
export function peekPendingMeetingStartRefusal(): PendingStartRefusal | null {
  return pendingStartRefusal;
}

/** Take and immediately clear any pending refused start handed off to RecordMeetingPage. */
export function consumePendingMeetingStartRefusal(): PendingStartRefusal | null {
  const current = pendingStartRefusal;
  pendingStartRefusal = null;
  return current;
}

/** Subscribe to app-wide refused-start hand-offs so App switches to Record a meeting. */
export function onMeetingStartRefusalHandOff(
  listener: (refusal: PendingStartRefusal) => void,
): () => void {
  startRefusalHandOffListeners.add(listener);
  return () => {
    startRefusalHandOffListeners.delete(listener);
  };
}

/**
 * Canonical Tauri event names shared with `src-tauri/src/meeting_pipeline.rs`.
 * Kept in one module and verified against Rust by `scripts/meeting-events-sync.test.ts`.
 */
export const MEETING_SEGMENT_EVENT = "meeting-segment" as const;
export const MEETING_STATE_EVENT = "meeting-state-changed" as const;
export const MEETINGS_OFFER_EVENT = "meetings-offer" as const;
export const MEETINGS_OPEN_CONSENT_EVENT = "meetings-open-consent" as const;
export const MEETING_NOTES_PROGRESS_EVENT = "meeting-notes-progress" as const;
export const MEETING_NOTES_SO_FAR_EVENT = "meeting-notes-so-far" as const;
export const MEETING_NOTES_READY_EVENT = "meeting-notes-ready" as const;
export const MEETING_NOTES_FAILED_EVENT = "meeting-notes-failed" as const;
export const MEETING_SPEAKERS_READY_EVENT = "meeting-speakers-ready" as const;
export const MEETING_EXTRAS_READY_EVENT = "meeting-extras-ready" as const;

export const ALL_MEETING_EVENTS = [
  MEETING_SEGMENT_EVENT,
  MEETING_STATE_EVENT,
  MEETINGS_OFFER_EVENT,
  MEETINGS_OPEN_CONSENT_EVENT,
  MEETING_NOTES_PROGRESS_EVENT,
  MEETING_NOTES_SO_FAR_EVENT,
  MEETING_NOTES_READY_EVENT,
  MEETING_NOTES_FAILED_EVENT,
  MEETING_SPEAKERS_READY_EVENT,
  MEETING_EXTRAS_READY_EVENT,
] as const;

export interface NotesProgressEvent {
  meeting_id: number;
  phase: string;
  elapsed_secs: number;
  estimate_secs: number | null;
  progress_label: string;
  calls_done: number;
  calls_total: number;
}

export interface NotesSoFarEvent {
  meeting_id: number;
  lines: MeetingNoteLineView[];
}

export interface NotesReadyEvent {
  meeting_id: number;
  time_to_notes_secs: number;
  title: string;
}

export interface NotesFailedEvent {
  meeting_id: number;
  reason: "no_model" | "unavailable" | "interrupted" | "failed" | string;
}

export interface SpeakersReadyEvent {
  meeting_id: number;
  speaker_count: number;
}

export interface ExtrasReadyEvent {
  meeting_id: number;
  left_out_count: number;
}
