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
