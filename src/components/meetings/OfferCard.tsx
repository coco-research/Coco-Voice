import React, { useId } from "react";
import { useTranslation } from "react-i18next";
import type { MeetingOffer } from "../../bindings.meetings";
import { offerBodyKey } from "./offerState";
import "./OfferCard.css";

const BRAND_NAME = "Coco Voice";
const NOT_RECORDING_RING = "\u25cb";

function PureOrbIcon({ gradientId }: { gradientId: string }) {
  return (
    <svg
      width={18}
      height={18}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <defs>
        <radialGradient id={gradientId} cx="38%" cy="32%" r="65%">
          <stop offset="0%" stopColor="#A9C4FF" />
          <stop offset="52%" stopColor="#5B8DFF" />
          <stop offset="100%" stopColor="#1D4ED8" />
        </radialGradient>
      </defs>
      <circle cx="32" cy="32" r="26" fill={`url(#${gradientId})`} />
      <circle cx="25" cy="23" r="8" fill="#EEF2FB" fillOpacity="0.38" />
      <circle
        cx="32"
        cy="32"
        r="26"
        stroke="#A9C4FF"
        strokeOpacity="0.45"
        strokeWidth="1.5"
      />
    </svg>
  );
}

export interface OfferCardProps {
  offer: MeetingOffer;
  onStart: () => void;
  onNotNow: () => void;
}

export function OfferCard({ offer, onStart, onNotNow }: OfferCardProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const bodyId = useId();
  const orbGradientId = useId();

  return (
    <section
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-describedby={bodyId}
      className="offer-card"
      data-testid="meeting-offer-card"
    >
      <div className="offer-brand-row">
        <PureOrbIcon gradientId={orbGradientId} />
        <span>{BRAND_NAME}</span>
      </div>
      <h2 id={titleId} className="offer-title">
        {t("meetings.offer.title")}
      </h2>
      <p id={bodyId} className="offer-body">
        {t(offerBodyKey(offer))}
      </p>
      <p className="offer-mark">
        <span className="offer-sh" aria-hidden="true">
          {NOT_RECORDING_RING}
        </span>
        <span>{t("meetings.offer.notRecording")}</span>
      </p>
      <div className="offer-acts">
        <button
          type="button"
          className="offer-btn offer-btn-accent"
          onClick={onStart}
        >
          {t("meetings.offer.start")}
        </button>
        <button type="button" className="offer-btn" onClick={onNotNow}>
          {t("meetings.offer.notNow")}
        </button>
      </div>
      <p className="offer-muted">{t("meetings.offer.neverOnItsOwn")}</p>
    </section>
  );
}
