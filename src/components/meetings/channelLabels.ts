import type { MeetingChannel } from "../../bindings.meetings";

// Mock 4 labels the two tracks "Me" and "Others". The words themselves live in
// the locale file; this only picks one, so the test needs no DOM.
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
