import { expect, test } from "bun:test";
import {
  startNotesFlow,
  type UpcomingMeeting,
} from "../src/components/meetings/upcoming";

// Synthetic meeting, never read from a real calendar.
const standup: UpcomingMeeting = {
  title: "Team standup",
  start: 1_800_000_000,
  end: 1_800_001_800,
  calendar_name: "Home",
  attendee_display_names: ["Ann", "Bo"],
};

function spyDeps(consent: () => Promise<boolean>) {
  const calls: string[] = [];
  return {
    calls,
    deps: {
      showConsentReminder: async () => {
        calls.push("consent");
        return consent();
      },
      keepForNotes: async () => {
        calls.push("keep");
      },
      beginNotes: async () => {
        calls.push("begin");
      },
    },
  };
}

test("start notes shows the consent reminder before any capture begins", async () => {
  const { calls, deps } = spyDeps(async () => true);
  expect(await startNotesFlow(standup, deps)).toBe("started");
  expect(calls).toEqual(["consent", "keep", "begin"]);
});

test("declining the consent reminder starts nothing and keeps nothing", async () => {
  const { calls, deps } = spyDeps(async () => false);
  expect(await startNotesFlow(standup, deps)).toBe("declined");
  expect(calls).toEqual(["consent"]);
});

test("a consent reminder that fails counts as a no", async () => {
  const { calls, deps } = spyDeps(async () => {
    throw new Error("dialog closed");
  });
  expect(await startNotesFlow(standup, deps)).toBe("declined");
  expect(calls).toEqual(["consent"]);
});

test("only an exact yes counts as consent", async () => {
  const { calls, deps } = spyDeps(async () => "yes" as unknown as boolean);
  expect(await startNotesFlow(standup, deps)).toBe("declined");
  expect(calls).toEqual(["consent"]);
});

test("with no consent reminder wired in, start notes captures nothing", async () => {
  const calls: string[] = [];
  const result = await startNotesFlow(standup, {
    keepForNotes: async () => void calls.push("keep"),
    beginNotes: async () => void calls.push("begin"),
  });
  expect(result).toBe("unavailable");
  expect(calls).toEqual([]);
});
