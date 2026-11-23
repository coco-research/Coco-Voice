import type { MeetingChannel } from "../../bindings.meetings";

export interface SegmentChannelLabels {
  me: string;
  others: string;
}

export function segmentChannelLabel(
  channel: MeetingChannel,
  labels: SegmentChannelLabels,
): string {
  return channel === "me" ? labels.me : labels.others;
}

function cleanUniqueNames(
  names?: ReadonlyArray<string | null | undefined> | null,
): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of names ?? []) {
    const trimmed = (raw ?? "").trim();
    if (!trimmed) continue;
    const lower = trimmed.toLowerCase();
    if (seen.has(lower)) continue;
    seen.add(lower);
    out.push(trimmed);
  }
  return out;
}

export function typedSpeakerNamesFromMap(
  speakerNames?: Record<string, string> | null,
): string[] {
  if (!speakerNames) return [];
  return Object.keys(speakerNames)
    .sort()
    .map((slot) => speakerNames[slot])
    .filter((name): name is string => Boolean(name && name.trim()));
}

/**
 * Shared I7 / AC-X23 participants line formatter used across every meeting header:
 * - Calendar attendees (current user first): "<first> with <rest>" ("Sam with Dana, Raj, Mei")
 * - Else typed speaker names: "Me with <names>" ("Me with Jordan")
 * - Else: "Me and Others"
 */
export function formatMeetingParticipantsLine(
  attendees: ReadonlyArray<string> | null | undefined,
  typedSpeakerNames: ReadonlyArray<string> | null | undefined,
  t: (key: string, opts?: Record<string, unknown>) => string,
  micOnly = false,
): string {
  const cleanAttendees = cleanUniqueNames(attendees);
  if (cleanAttendees.length >= 2) {
    return t("meetings.view.participantsWith", {
      first: cleanAttendees[0],
      others: cleanAttendees.slice(1).join(", "),
    });
  }
  if (cleanAttendees.length === 1) {
    return cleanAttendees[0];
  }
  const cleanTyped = cleanUniqueNames(typedSpeakerNames);
  if (cleanTyped.length > 0) {
    return t("meetings.view.meWithSpeakers", {
      names: cleanTyped.join(", "),
    });
  }
  if (micOnly) {
    return t("meetings.view.channel.me");
  }
  return t("meetings.view.channels");
}
