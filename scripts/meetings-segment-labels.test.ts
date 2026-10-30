import { describe, expect, test } from "bun:test";
import en from "../src/i18n/locales/en/translation.json";
import { segmentChannelLabel } from "../src/components/meetings/channelLabels";

// PR6c: the live list labels the two tracks "Me" and "Others" in mock 4. The
// words come from the locale file, so this reads the English one.
describe("segment_channel_labels", () => {
  const labels = en.meetings.view.channel;

  test("segment_channel_labels maps me to Me and others to Others", () => {
    expect(segmentChannelLabel("me", labels)).toBe("Me");
    expect(segmentChannelLabel("others", labels)).toBe("Others");
  });
});
