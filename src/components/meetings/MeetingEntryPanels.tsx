import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { MeetingOffer, MeetingState } from "../../bindings.meetings";

const DOT_FILLED = "●";
const DOT_OPEN = "○";

export function formatMeetingClock(seconds: number): string {
  const safe = Math.max(0, Math.floor(seconds));
  const mins = String(Math.floor(safe / 60)).padStart(2, "0");
  const secs = String(safe % 60).padStart(2, "0");
  return `${mins}:${secs}`;
}

// Only an explicit Stop action (or idle state) may hide the meeting pill;
// delayed dictation overlay hides must never take down an active meeting pill.
export function shouldHideMeetingPill(
  trigger: "stop" | "dictation_hide",
  state: MeetingState,
): boolean {
  if (state === "recording" || state === "paused") {
    return trigger === "stop";
  }
  return true;
}

function offerReasonKey(offer: MeetingOffer): string {
  if (offer.app === "teams") {
    return offer.inCall
      ? "meetings.offer.teamsInCall"
      : "meetings.offer.teamsOpen";
  }
  if (offer.app === "meet") {
    return "meetings.offer.meet";
  }
  return offer.inCall ? "meetings.offer.zoomInCall" : "meetings.offer.zoomOpen";
}

export const OfferPanel: React.FC<{
  offer: MeetingOffer;
  onStartNotes: () => void;
  onDismiss: (bundleId: string) => void;
}> = ({ offer, onStartNotes, onDismiss }) => {
  const { t } = useTranslation();
  return (
    <section
      data-testid="frame-1-offer"
      aria-label={t("meetings.offer.title")}
      className="rounded-[14px] border border-mid-gray/30 bg-background p-3.5 shadow-sm space-y-2"
    >
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[15px] font-semibold text-text">
          {t("meetings.offer.title")}
        </h3>
        <span className="inline-flex items-center gap-1 text-[11.5px] font-semibold text-mid-gray">
          <span aria-hidden="true">{DOT_OPEN}</span>
          <span>{t("meetings.offer.notRecording")}</span>
        </span>
      </div>
      <p className="text-[13px] text-text">{t(offerReasonKey(offer))}</p>
      <p className="text-[12px] text-mid-gray">
        {t("meetings.offer.neverOnItsOwn")}
      </p>
      <div className="flex items-center gap-2 pt-1">
        <button
          type="button"
          onClick={onStartNotes}
          className="inline-flex min-h-[30px] items-center rounded-[9px] bg-logo-primary px-3 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          {t("meetings.offer.start")}
        </button>
        <button
          type="button"
          onClick={() => onDismiss(offer.bundleId)}
          className="inline-flex min-h-[30px] items-center rounded-[9px] border border-mid-gray/40 px-3 text-[12.5px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.offer.notNow")}
        </button>
      </div>
    </section>
  );
};

export const ConsentDialog: React.FC<{
  open: boolean;
  starting?: boolean;
  errorMessage?: string | null;
  onConfirmStart: () => void;
  onCancel: () => void;
  onCopyAnnouncement?: (text: string) => void;
}> = ({
  open,
  starting = false,
  errorMessage = null,
  onConfirmStart,
  onCancel,
  onCopyAnnouncement,
}) => {
  const { t } = useTranslation();
  const [toldEveryone, setToldEveryone] = useState(false);

  if (!open) return null;

  const announcement = t("meetings.consent.announcement");
  const handleCopy = () => {
    if (onCopyAnnouncement) {
      onCopyAnnouncement(announcement);
      return;
    }
    void navigator?.clipboard?.writeText?.(announcement);
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("meetings.consent.title")}
      data-testid="frame-2-consent"
      className="rounded-[14px] border border-mid-gray/35 bg-background p-4 shadow-md space-y-3"
    >
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[16px] font-semibold text-text">
          {t("meetings.consent.title")}
        </h3>
        <span className="inline-flex items-center gap-1 text-[11.5px] font-semibold text-mid-gray">
          <span aria-hidden="true">{DOT_OPEN}</span>
          <span>{t("meetings.offer.notRecording")}</span>
        </span>
      </div>
      <p className="text-[13px] leading-[19px] text-text">
        {t("meetings.consent.reminder")}
      </p>
      <div className="rounded-[10px] border border-mid-gray/25 bg-mid-gray/[0.06] p-2.5 space-y-2">
        <p className="text-[12.5px] italic text-text">{announcement}</p>
        <button
          type="button"
          onClick={handleCopy}
          className="inline-flex min-h-[26px] items-center rounded-[7px] border border-mid-gray/45 bg-background px-2.5 text-[12px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.consent.copyAnnouncement")}
        </button>
      </div>
      <label className="flex items-center gap-2 text-[13px] text-text cursor-pointer">
        <input
          type="checkbox"
          checked={toldEveryone}
          onChange={(e) => setToldEveryone(e.target.checked)}
        />
        <span>{t("meetings.consent.toldEveryone")}</span>
      </label>
      <p className="text-[12px] text-mid-gray">
        {t("meetings.consent.orgRules")}
      </p>
      {errorMessage ? (
        <p role="alert" className="text-[12px] text-red-600">
          {errorMessage}
        </p>
      ) : null}
      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={onCancel}
          className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[13px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.consent.cancel")}
        </button>
        <button
          type="button"
          disabled={starting}
          onClick={onConfirmStart}
          className="inline-flex min-h-[32px] items-center gap-1.5 rounded-[9px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
        >
          <span aria-hidden="true">{DOT_FILLED}</span>
          <span>{t("meetings.consent.start")}</span>
        </button>
      </div>
    </div>
  );
};

export const MeetingFloatingPill: React.FC<{
  elapsedSecs: number;
  paused?: boolean;
  pausedLabel?: string;
  resumeLabel?: string;
  onResume?: () => void;
  onStop: () => void;
}> = ({
  elapsedSecs,
  paused = false,
  pausedLabel,
  resumeLabel,
  onResume,
  onStop,
}) => {
  const { t } = useTranslation();
  const clock = formatMeetingClock(elapsedSecs);
  const dot = paused ? DOT_OPEN : DOT_FILLED;
  const statusLabel = paused
    ? (pausedLabel ?? t("meetings.pill.recording"))
    : t("meetings.pill.recording");

  return (
    <div
      data-testid="frame-3-pill"
      role="status"
      aria-live="polite"
      className="inline-flex items-center gap-2.5 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-md"
    >
      <span
        aria-hidden="true"
        className={paused ? "text-amber-600" : "text-red-600"}
      >
        {dot}
      </span>
      <span className="text-[12.5px] font-semibold text-text">
        {statusLabel}
      </span>
      <span className="font-mono text-[12.5px] tabular-nums text-text">
        {clock}
      </span>
      {paused && onResume && resumeLabel ? (
        <button
          type="button"
          onClick={onResume}
          className="rounded-full border border-mid-gray/40 px-2.5 py-0.5 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {resumeLabel}
        </button>
      ) : null}
      <button
        type="button"
        onClick={onStop}
        className="rounded-full bg-text px-2.5 py-0.5 text-[12px] font-semibold text-background cursor-pointer hover:opacity-90"
      >
        {t("meetings.pill.stop")}
      </button>
    </div>
  );
};
