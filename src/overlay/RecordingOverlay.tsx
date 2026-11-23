import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  CleanupProgress,
  CleanupStatus,
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import { CleanupMark } from "@/components/shared/CleanupMark";
import {
  AgentAskPillView,
  HandsFreePillView,
  MeetingPillView,
  RecordingPillShowcase,
  RecordingPillStateView,
  type RecordingPillKind,
} from "./RecordingPillStates";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { CLEANUP_NOTE } from "@/lib/utils/cleanupStatus";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState =
  "recording" | "streaming" | "transcribing" | "processing" | "meeting";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as 16 FFT buckets; we take the first N.
const WAVE_BARS = 9;

// Reduced motion batches word updates. 250 ms is 4 Hz.
const REDUCED_MOTION_GAP_MS = 250;

type PolishSnapshot = { text: string; done: boolean };

function overlayWorkLabel(
  polishing: boolean,
  onDevice: boolean,
  t: (
    key: "overlay.transcribing" | "overlay.processing" | "overlay.polishing",
  ) => string,
): string {
  if (!polishing) return t("overlay.transcribing");
  return t(onDevice ? "overlay.polishing" : "overlay.processing");
}

// The status row for a take where cleanup did not run: the mark, then a few words
// that never wrap and never end in an ellipsis. If the words do not fit the line,
// only the short form shows; the reason is in the tooltip and in Settings.
const CleanupChip: React.FC<{ status: CleanupStatus }> = ({ status }) => {
  const { t } = useTranslation();
  const wordsRef = useRef<HTMLSpanElement>(null);
  const [short, setShort] = useState(false);
  const note = CLEANUP_NOTE[status.reason];
  const announce = t("cleanupStatus.announce", {
    sentence: t(`cleanupStatus.sentence.${note}`),
  });
  const fallback = status.needs_action
    ? "fallbackNeedsYou"
    : "fallbackThisTake";

  useLayoutEffect(() => {
    const el = wordsRef.current;
    if (el && el.scrollWidth > el.clientWidth) setShort(true);
  }, []);

  return (
    <div
      className={`sbase schip ${status.needs_action ? "needs-you" : "quiet"}`}
      role="status"
      title={announce}
      aria-label={announce}
    >
      <CleanupMark needsAction={status.needs_action} />
      <span className="schip-words" ref={wordsRef}>
        {t(`cleanupStatus.chip.${short ? fallback : note}`)}
      </span>
    </div>
  );
};

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  // Why cleanup did not run for this take; cleared when the next recording starts.
  const [cleanupStatus, setCleanupStatus] = useState<CleanupStatus | null>(
    null,
  );
  // On-device cleanup is streaming into the overlay. Cloud never sets this.
  const [onDevicePolish, setOnDevicePolish] = useState(false);
  const [polish, setPolish] = useState<PolishSnapshot | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [meetingStartedAtMs, setMeetingStartedAtMs] = useState<number | null>(
    null,
  );
  const [meetingElapsedSecs, setMeetingElapsedSecs] = useState(0);
  const [meetingStopping, setMeetingStopping] = useState(false);
  const [meetingMicOnly, setMeetingMicOnly] = useState(false);
  const [handsFreeMode, setHandsFreeMode] = useState<{
    hands_free: boolean;
    key?: string;
  } | null>(null);
  const [agentAskQuestion, setAgentAskQuestion] = useState<{
    agent_name: string;
    question: string;
    queued_count?: number;
    shortcut_key?: string;
  } | null>(null);
  const [agentAnswerSentTo, setAgentAnswerSentTo] = useState<string | null>(
    null,
  );
  const [pillOutcome, setPillOutcome] = useState<{
    kind: RecordingPillKind;
    targetApp?: string;
  } | null>(null);
  const [showcaseAllStates, setShowcaseAllStates] = useState(false);
  // Bumped on each new streaming session so the Live card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);
  // Overlay placement (top vs bottom of the screen). The Live panel grows downward
  // from a top overlay (oldest line under the pill) and upward from a bottom one.
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp flush under the pill.
  const [overflowing, setOverflowing] = useState(false);

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const stateRef = useRef<OverlayState>("recording");
  const phaseRef = useRef<StreamPhase>("listening");
  const onDeviceRef = useRef(false);
  const reducedMotionRef = useRef(false);
  const polishTimerRef = useRef<number | null>(null);
  const agentSentTimerRef = useRef<number | null>(null);
  const lastPolishApplyRef = useRef(0);
  const pendingPolishRef = useRef<PolishSnapshot | null>(null);
  const resetPolishRef = useRef(() => {});
  const schedulePolishRef = useRef<(next: PolishSnapshot) => void>(() => {});
  const freshTakeCapturing = () =>
    stateRef.current === "recording" ||
    (stateRef.current === "streaming" && phaseRef.current === "listening");
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    resetPolishRef.current = () => {
      onDeviceRef.current = false;
      setOnDevicePolish(false);
      setPolish(null);
      pendingPolishRef.current = null;
      lastPolishApplyRef.current = 0;
      if (polishTimerRef.current !== null) {
        window.clearTimeout(polishTimerRef.current);
        polishTimerRef.current = null;
      }
    };

    schedulePolishRef.current = (next) => {
      const apply = (snapshot: PolishSnapshot) => {
        if (polishTimerRef.current !== null) {
          window.clearTimeout(polishTimerRef.current);
          polishTimerRef.current = null;
        }
        pendingPolishRef.current = null;
        lastPolishApplyRef.current = performance.now();
        setPolish(snapshot);
      };
      if (next.done || !reducedMotionRef.current) {
        apply(next);
        return;
      }
      const gap = performance.now() - lastPolishApplyRef.current;
      if (lastPolishApplyRef.current === 0 || gap >= REDUCED_MOTION_GAP_MS) {
        apply(next);
        return;
      }
      pendingPolishRef.current = next;
      if (polishTimerRef.current === null) {
        polishTimerRef.current = window.setTimeout(() => {
          polishTimerRef.current = null;
          const pending = pendingPolishRef.current;
          pendingPolishRef.current = null;
          if (pending) apply(pending);
        }, REDUCED_MOTION_GAP_MS - gap);
      }
    };
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => {
      reducedMotionRef.current = media.matches;
    };
    sync();
    media.addEventListener("change", sync);
    return () => {
      media.removeEventListener("change", sync);
      if (polishTimerRef.current !== null) {
        window.clearTimeout(polishTimerRef.current);
        polishTimerRef.current = null;
      }
      if (agentSentTimerRef.current !== null) {
        window.clearTimeout(agentSentTimerRef.current);
        agentSentTimerRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShowcase = await listen("show-pill-showcase", () => {
        setShowcaseAllStates(true);
        setIsVisible(true);
      });
      const unlistenLegacyStream = await listen("stream-text", () => {
        setShowcaseAllStates(true);
        setIsVisible(true);
      });
      const unlistenOutcome = await listen<{
        kind: RecordingPillKind;
        targetApp?: string;
      } | null>("pill-outcome", (event) => {
        setShowcaseAllStates(false);
        setPillOutcome(event.payload ?? null);
        if (event.payload) setIsVisible(true);
      });
      const unlistenPillMode = await listen<{
        hands_free: boolean;
        key?: string;
      }>("pill-mode", (event) => {
        setHandsFreeMode(event.payload?.hands_free ? event.payload : null);
      });
      const unlistenMeetingPill = await listen<{
        started_at_ms: number;
        mic_only?: boolean;
      }>("meeting-pill", (event) => {
        const startedAt = event.payload?.started_at_ms;
        if (typeof startedAt === "number" && startedAt > 0) {
          setMeetingStartedAtMs(startedAt);
          setMeetingElapsedSecs(
            Math.max(0, Math.floor((Date.now() - startedAt) / 1000)),
          );
          setMeetingStopping(false);
        }
        if (typeof event.payload?.mic_only === "boolean") {
          setMeetingMicOnly(event.payload.mic_only);
        } else {
          void invoke<{ mic_only?: boolean }>("meeting_status")
            .then((st) => setMeetingMicOnly(Boolean(st?.mic_only)))
            .catch(() => {});
        }
      });
      const unlistenShow = await listen("show-overlay", async (event) => {
        setPillOutcome(null);
        const overlayState = event.payload as OverlayState;
        stateRef.current = overlayState;
        setState(overlayState);
        if (overlayState === "recording" || overlayState === "streaming") {
          setStreamText({ committed: "", tentative: "" });
          setCleanupStatus(null);
          resetPolishRef.current();
        }
        if (overlayState === "meeting") {
          setMeetingStopping(false);
          setMeetingStartedAtMs((prev) => prev ?? Date.now());
          void invoke<{ mic_only?: boolean }>("meeting_status")
            .then((st) => setMeetingMicOnly(Boolean(st?.mic_only)))
            .catch(() => {});
        }
        if (overlayState === "streaming") {
          phaseRef.current = "listening";
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setSession((s) => s + 1);
        }
        setIsVisible(true);
        await syncLanguageFromSettings();
        try {
          const settings = await commands.getAppSettings();
          if (settings.status === "ok") {
            setPosition(
              settings.data.overlay_position === "top" ? "top" : "bottom",
            );
          }
        } catch {
          // Keep the previous/default placement if settings can't be read.
        }
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        setAgentAskQuestion(null);
        setAgentAnswerSentTo(null);
        setPillOutcome(null);
        setHandsFreeMode(null);
        setMeetingStopping(false);
        setMeetingStartedAtMs(null);
        setMeetingMicOnly(false);
        setShowcaseAllStates(false);
      });

      const unlistenAgentAsk = await listen<{
        agent_name: string;
        question: string;
        queued_count?: number;
        shortcut_key?: string;
      } | null>("agent-ask-question", (event) => {
        setShowcaseAllStates(false);
        setAgentAskQuestion(event.payload ?? null);
        if (event.payload) setIsVisible(true);
      });

      const unlistenAgentAnswered = await listen<{ agent_name: string }>(
        "agent-ask-answered",
        (event) => {
          const agent = event.payload?.agent_name;
          if (!agent) return;
          setAgentAskQuestion(null);
          setAgentAnswerSentTo(agent);
          setIsVisible(true);
          if (agentSentTimerRef.current !== null) {
            window.clearTimeout(agentSentTimerRef.current);
          }
          agentSentTimerRef.current = window.setTimeout(() => {
            agentSentTimerRef.current = null;
            setAgentAnswerSentTo(null);
          }, 2000);
        },
      );

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, WAVE_BARS));
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        phaseRef.current = payload.phase;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      const unlistenCleanup = await events.cleanupStatus.listen((event) => {
        if (freshTakeCapturing()) return;
        setCleanupStatus(event.payload);
        resetPolishRef.current();
      });

      const unlistenProgress = await events.cleanupProgress.listen((event) => {
        if (freshTakeCapturing()) return;
        const payload: CleanupProgress = event.payload;
        if (!onDeviceRef.current) {
          onDeviceRef.current = true;
          setOnDevicePolish(true);
        }
        if (stateRef.current !== "streaming") return;
        if (payload.done || payload.text.trim().length > 0) {
          schedulePolishRef.current({ text: payload.text, done: payload.done });
        }
      });

      return () => {
        unlistenShowcase();
        unlistenLegacyStream();
        unlistenOutcome();
        unlistenPillMode();
        unlistenMeetingPill();
        unlistenShow();
        unlistenHide();
        unlistenAgentAsk();
        unlistenAgentAnswered();
        unlistenLevel();
        unlistenStream();
        unlistenPhase();
        unlistenCleanup();
        unlistenProgress();
      };
    };

    void setupEventListeners();
  }, []);

  // Elapsed timer while the Live overlay is visible.
  useEffect(() => {
    if (state !== "streaming" || !isVisible) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible]);

  // Drift-free local counter for the Meeting pill (counts from started_at_ms).
  useEffect(() => {
    if (state !== "meeting" || !isVisible || meetingStartedAtMs === null) {
      return;
    }
    const update = () => {
      setMeetingElapsedSecs(
        Math.max(0, Math.floor((Date.now() - meetingStartedAtMs) / 1000)),
      );
    };
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [state, isVisible, meetingStartedAtMs]);

  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText, polish, onDevicePolish]);

  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const handleStopMeeting = () => {
    if (meetingStopping) return;
    setMeetingStopping(true);
    void invoke("stop_meeting").catch(() => {
      setMeetingStopping(false);
    });
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  const waveform = (
    <div className="swave">
      {levels.map((v, i) => (
        <i
          key={i}
          style={{
            height: `${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px`,
          }}
        />
      ))}
    </div>
  );

  const cancelBtn = (
    <button
      className="sx"
      aria-label={t("tray.cancel")}
      onClick={() => commands.cancelOperation()}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M4 4 L12 12 M12 4 L4 12"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );

  const listeningRow = (showTimer: boolean, showCancel: boolean) => (
    <div
      className="sbase"
      role="status"
      aria-live="polite"
      aria-label={t("overlay.pillStates.listening", "Listening")}
    >
      <div className="sbase-l">
        <span className="sdot" />
      </div>
      {waveform}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  const spinnerRow = (label: string, showCancel: boolean, announce = false) => (
    <div
      className="sbase"
      role={announce ? "status" : undefined}
      aria-live={announce ? "polite" : undefined}
      aria-atomic={announce ? true : undefined}
    >
      <div className="sbase-l">
        <span className="sspinner" />
      </div>
      <span className="swork-label">{label}</span>
      <div className="sbase-r">{showCancel && cancelBtn}</div>
    </div>
  );

  const cleanupRow = cleanupStatus && (
    <CleanupChip key={cleanupStatus.reason} status={cleanupStatus} />
  );

  const workingRow = (label: string, showCancel: boolean, announce = false) =>
    cleanupRow ?? spinnerRow(label, showCancel, announce);

  if (showcaseAllStates) {
    return (
      <div
        dir={direction}
        className="flex min-h-screen w-full items-center justify-center"
      >
        <RecordingPillShowcase />
      </div>
    );
  }

  if (pillOutcome) {
    return (
      <div dir={direction} className={`ov-stage ${position} ov-fade show`}>
        <RecordingPillStateView
          kind={pillOutcome.kind}
          targetApp={pillOutcome.targetApp}
        />
      </div>
    );
  }

  if (state === "meeting") {
    const waitingCount = agentAskQuestion
      ? 1 + (agentAskQuestion.queued_count ?? 0)
      : 0;
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <MeetingPillView
          elapsedSecs={meetingElapsedSecs}
          stopping={meetingStopping}
          micOnly={meetingMicOnly}
          waitingAgentQuestions={waitingCount}
          onStop={handleStopMeeting}
        />
      </div>
    );
  }

  if (agentAnswerSentTo) {
    return (
      <div dir={direction} className={`ov-stage ${position} ov-fade show`}>
        <AgentAskPillView
          agentName={agentAnswerSentTo}
          question=""
          answerSentTo={agentAnswerSentTo}
        />
      </div>
    );
  }

  if (agentAskQuestion) {
    return (
      <div dir={direction} className={`ov-stage ${position} ov-fade show`}>
        <AgentAskPillView
          agentName={agentAskQuestion.agent_name}
          question={agentAskQuestion.question}
          shortcutKey={agentAskQuestion.shortcut_key}
          queuedCount={agentAskQuestion.queued_count ?? 0}
        />
      </div>
    );
  }

  if (state === "streaming") {
    const polishingView = phase === "working" && onDevicePolish;
    const cleanedNow = polish?.text ?? "";
    const hasCleaned = cleanedNow.trim().length > 0;
    const showCleaned = polishingView && hasCleaned;
    const hasText =
      streamText.committed.length > 0 ||
      streamText.tentative.length > 0 ||
      hasCleaned;
    const working = phase === "working";
    const open = hasText;
    const collapsed = working && !hasText;

    return (
      <div dir={direction} className={`ov-stage ${position}`}>
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""} ${
            cleanupStatus ? "with-chip" : ""
          } ${isVisible ? "" : "leaving"}`}
        >
          <div className="stext" aria-busy={polishingView ? true : undefined}>
            <div className="stext-clip">
              <div
                className={`stext-cap ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
              >
                <p>
                  {showCleaned ? (
                    cleanedNow
                  ) : (
                    <>
                      <span className={polishingView ? "raw" : "committed"}>
                        {streamText.committed ? streamText.committed + " " : ""}
                      </span>
                      <span className={polishingView ? "raw" : "tentative"}>
                        {streamText.tentative}
                      </span>
                      {!working && <span className="scaret" />}
                    </>
                  )}
                </p>
              </div>
            </div>
          </div>
          {working
            ? workingRow(
                overlayWorkLabel(workKind === "polishing", onDevicePolish, t),
                true,
                workKind === "polishing" && onDevicePolish,
              )
            : listeningRow(open, true)}
        </div>
      </div>
    );
  }

  const working = state === "transcribing" || state === "processing";

  if (working && !cleanupStatus) {
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <RecordingPillStateView kind="working" />
      </div>
    );
  }

  if (!working && handsFreeMode?.hands_free) {
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <HandsFreePillView shortcutKey={handsFreeMode.key || "fn"} />
      </div>
    );
  }

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
    >
      <div
        className={`scard compact ${
          working && (isVisible || cleanupStatus) ? "cworking" : ""
        } ${cleanupStatus ? "with-chip" : ""}`}
      >
        {working
          ? workingRow(
              overlayWorkLabel(state === "processing", onDevicePolish, t),
              true,
              state === "processing" && onDevicePolish,
            )
          : listeningRow(false, true)}
      </div>
    </div>
  );
};

export default RecordingOverlay;
