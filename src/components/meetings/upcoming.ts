import type { UpcomingEvent } from "../../bindings.meetings";

export type UpcomingMeeting = UpcomingEvent;

/**
 * What "Start notes" needs from the rest of the app. The consent reminder is
 * required: there is no way to begin notes without it answering yes first.
 */
export interface StartNotesDeps {
  /** Shows the meeting consent reminder. Resolves true only if the person confirms. */
  showConsentReminder: (meeting: UpcomingMeeting) => Promise<boolean>;
  /** Tells the app this meeting stays in memory because notes are starting. */
  keepForNotes: (meeting: UpcomingMeeting) => Promise<unknown>;
  /** Begins capture and notes. Only ever called after a yes. */
  beginNotes: (meeting: UpcomingMeeting) => Promise<void> | void;
}

export type StartNotesResult = "started" | "declined" | "unavailable";

export async function startNotesFlow(
  meeting: UpcomingMeeting,
  deps: Partial<StartNotesDeps>,
): Promise<StartNotesResult> {
  const { showConsentReminder, keepForNotes, beginNotes } = deps;
  // Fails closed: with no reminder wired in, nothing is captured.
  if (!showConsentReminder || !keepForNotes || !beginNotes) {
    return "unavailable";
  }
  let confirmed = false;
  try {
    confirmed = (await showConsentReminder(meeting)) === true;
  } catch {
    confirmed = false;
  }
  if (!confirmed) return "declined";
  await keepForNotes(meeting);
  await beginNotes(meeting);
  return "started";
}

/** Wiring point for the app's consent reminder and capture start. Empty until registered. */
let wired: Partial<Pick<StartNotesDeps, "showConsentReminder" | "beginNotes">> =
  {};

export function wireStartNotes(
  deps: Pick<StartNotesDeps, "showConsentReminder" | "beginNotes">,
): void {
  wired = deps;
}

export function currentStartNotesDeps(): Partial<
  Pick<StartNotesDeps, "showConsentReminder" | "beginNotes">
> {
  return wired;
}
