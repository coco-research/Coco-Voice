import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands } from "../../bindings.meetings";
import type {
  MeetingSegmentView,
  MeetingStatus,
  MeetingWarning,
} from "../../bindings.meetings";
import { Button } from "../ui/Button";
import { segmentChannelLabel } from "./channelLabels";

// Same slack the overlay uses: within ~a line of the bottom counts as pinned.
const PIN_SLACK_PX = 16;

function formatOffset(ms: number): string {
  const total = Math.floor(ms / 1000);
  const minutes = String(Math.floor(total / 60)).padStart(2, "0");
  const seconds = String(total % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

function sameSegment(a: MeetingSegmentView, b: MeetingSegmentView): boolean {
  return (
    a.meeting_id === b.meeting_id &&
    a.channel === b.channel &&
    a.start_ms === b.start_ms &&
    a.end_ms === b.end_ms
  );
}

// Oldest first, no duplicates. A segment can arrive on the event before the
// stored list is read, so the two sources are merged.
// ponytail: linear scan per segment; a meeting has one row per chunk, so small.
function mergeSegments(
  current: MeetingSegmentView[],
  incoming: MeetingSegmentView[],
): MeetingSegmentView[] {
  const next = [...current];
  for (const segment of incoming) {
    if (!next.some((existing) => sameSegment(existing, segment))) {
      next.push(segment);
    }
  }
  return next.sort((a, b) => a.start_ms - b.start_ms);
}

function warningKey(warning: MeetingWarning): string {
  return warning === "system_audio_silent"
    ? "meetings.view.warnings.systemAudioSilent"
    : "meetings.view.warnings.micFellBackToDefault";
}

// The mock's half-filled circle: filled while the list follows the newest line.
function AutoScrollMark({ paused }: { paused: boolean }) {
  return (
    <svg viewBox="0 0 12 12" className="h-3 w-3" aria-hidden="true">
      <circle
        cx="6"
        cy="6"
        r="4.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      {!paused && <path d="M6 1.5A4.5 4.5 0 0 1 6 10.5Z" fill="currentColor" />}
    </svg>
  );
}

export function MeetingsView() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<MeetingStatus | null>(null);
  const [segments, setSegments] = useState<MeetingSegmentView[]>([]);
  const [elapsed, setElapsed] = useState(0);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [following, setFollowing] = useState(true);
  const [loading, setLoading] = useState(true);
  const [stopping, setStopping] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);

  const meetingIdRef = useRef<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const applyStatus = useCallback((next: MeetingStatus) => {
    if (next.meeting_id !== meetingIdRef.current) {
      meetingIdRef.current = next.meeting_id;
      setSegments([]);
      setFollowing(true);
      setErrorKey(null);
      setStartedAt(null);
      if (next.meeting_id !== null) {
        setStartedAt(Date.now() - next.elapsed_secs * 1000);
      }
      setElapsed(next.elapsed_secs);
    } else if (next.state === "recording" || next.state === "stopping") {
      // Only the live clock moves. After Stop the last value stays on screen.
      setElapsed(next.elapsed_secs);
    }
    setStatus(next);
  }, []);

  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    // The status and stored segments are read after the listeners are up, so
    // anything that arrives in between waits here and is merged afterwards.
    let ready = false;
    const queued: MeetingSegmentView[] = [];

    const read = async () => {
      const registrations = await Promise.all([
        listen<MeetingSegmentView>("meeting-segment", (event) => {
          if (disposed) return;
          if (!ready) {
            queued.push(event.payload);
            return;
          }
          if (event.payload.meeting_id !== meetingIdRef.current) return;
          setSegments((current) => mergeSegments(current, [event.payload]));
        }),
        listen<MeetingStatus>("meeting-state-changed", (event) => {
          if (!disposed) applyStatus(event.payload);
        }),
      ]);
      if (disposed) {
        for (const stop of registrations) stop();
        return;
      }
      stops.push(...registrations);

      try {
        const current = await commands.meetingStatus();
        if (disposed) return;
        applyStatus(current);
        if (current.meeting_id !== null) {
          const stored = await commands.meetingSegments(current.meeting_id);
          if (disposed) return;
          if (stored.status === "ok") {
            setSegments((previous) => mergeSegments(previous, stored.data));
          } else {
            setErrorKey("meetings.view.loadFailed");
          }
        }
      } catch (loadError) {
        console.error("Could not read the meeting", loadError);
        if (!disposed) setErrorKey("meetings.view.loadFailed");
      } finally {
        ready = true;
        setLoading(false);
        if (!disposed && queued.length > 0) {
          const meetingId = meetingIdRef.current;
          const buffered = queued.filter(
            (segment) => segment.meeting_id === meetingId,
          );
          setSegments((previous) => mergeSegments(previous, buffered));
        }
      }
    };

    void read().catch((loadError) => {
      console.error("Could not read the meeting", loadError);
      if (!disposed) {
        setErrorKey("meetings.view.loadFailed");
        setLoading(false);
      }
    });

    return () => {
      disposed = true;
      for (const stop of stops) stop();
    };
  }, [applyStatus]);

  // Follow the newest line unless the reader scrolled up to read an older one.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && following) list.scrollTop = list.scrollHeight;
  }, [segments, following]);

  const handleScroll = () => {
    const list = listRef.current;
    if (!list) return;
    setFollowing(
      list.scrollHeight - list.scrollTop - list.clientHeight <= PIN_SLACK_PX,
    );
  };

  const stop = async () => {
    setStopping(true);
    setErrorKey(null);
    try {
      const result = await commands.stopMeeting();
      if (result.status === "ok") {
        applyStatus(result.data);
      } else {
        setErrorKey("meetings.view.stopFailed");
      }
    } catch (stopError) {
      console.error("Could not stop the meeting", stopError);
      setErrorKey("meetings.view.stopFailed");
    } finally {
      setStopping(false);
    }
  };

  if (loading && status === null) {
    return <div className="max-w-3xl w-full mx-auto" />;
  }

  if (status === null || status.meeting_id === null || startedAt === null) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <p className="text-sm text-mid-gray">
          {t(errorKey ?? "meetings.view.noMeeting")}
        </p>
      </div>
    );
  }

  const recording = status.state === "recording" || status.state === "stopping";
  const startTime = new Date(startedAt).toLocaleTimeString(i18n.language, {
    hour: "numeric",
    minute: "2-digit",
  });
  const channelLabels = {
    me: t("meetings.view.channel.me"),
    others: t("meetings.view.channel.others"),
  };
  const stateLabel = recording
    ? t("meetings.view.recording")
    : t("meetings.view.finished");
  const scrollLabel = following
    ? t("meetings.view.autoScrolling")
    : t("meetings.view.scrollPaused");
  const dotClass = recording
    ? "h-2 w-2 rounded-full animate-pulse bg-red-500"
    : "h-2 w-2 rounded-full bg-mid-gray";

  return (
    <div className="max-w-3xl w-full mx-auto">
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold text-text">
            {t("meetings.view.title", { time: startTime })}
          </h1>
          <p className="mt-0.5 text-xs text-mid-gray">
            {t("meetings.view.channels")}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <span className="flex items-center gap-1.5 text-sm font-medium">
            <span className={dotClass} />
            {stateLabel}
          </span>
          <span className="font-mono text-sm tabular-nums text-mid-gray">
            {formatOffset(elapsed * 1000)}
          </span>
          {recording && (
            <Button
              variant="secondary"
              size="sm"
              disabled={stopping || status.state === "stopping"}
              onClick={() => void stop()}
            >
              {t("meetings.view.stop")}
            </Button>
          )}
        </div>
      </header>

      {status.mic_name !== null && (
        <p className="mt-1 text-xs text-mid-gray">
          {t("meetings.view.mic", { name: status.mic_name })}
        </p>
      )}

      {status.warnings.length > 0 && (
        <div className="mt-3 space-y-1 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
          {status.warnings.map((warning) => (
            <p key={warning}>{t(warningKey(warning))}</p>
          ))}
        </div>
      )}

      {errorKey !== null && (
        <p className="mt-3 leading-5 text-sm text-red-600" role="alert">
          {t(errorKey)}
        </p>
      )}

      <section className="mt-4">
        <div className="mb-1 flex min-h-5 items-center justify-end">
          {segments.length > 0 && (
            <div className="flex select-none items-center gap-1.5 text-xs text-mid-gray">
              <AutoScrollMark paused={!following} />
              {scrollLabel}
            </div>
          )}
        </div>
        <div
          ref={listRef}
          onScroll={handleScroll}
          className="max-h-[60vh] min-h-40 overflow-y-auto rounded-lg border border-mid-gray/20 bg-background"
        >
          <ol>
            {segments.map((segment) => (
              <li
                key={`${segment.channel}-${segment.start_ms}-${segment.end_ms}`}
                className="flex gap-3 border-b border-mid-gray/10 px-3 py-2 text-sm last:border-b-0"
              >
                <span className="w-10 shrink-0 font-mono text-xs leading-5 tabular-nums text-mid-gray">
                  {formatOffset(segment.start_ms)}
                </span>
                <span className="w-14 shrink-0 text-xs font-semibold leading-5 text-mid-gray">
                  {segmentChannelLabel(segment.channel, channelLabels)}
                </span>
                <span className="min-w-0 flex-1 leading-5 text-text">
                  {segment.text}
                </span>
              </li>
            ))}
          </ol>
        </div>
      </section>

      <footer className="mt-3 space-y-1 text-xs text-mid-gray">
        <p>{t("meetings.view.hints.headphones")}</p>
        <p>{t("meetings.view.hints.dictation")}</p>
      </footer>
    </div>
  );
}
