import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Users } from "lucide-react";
import { platform, version } from "@tauri-apps/plugin-os";
import { MeetingsToggle } from "../settings/MeetingsToggle";
import type { MeetingsUi } from "./gate";

export const MEETINGS_UI_SENTINEL = "coco-meetings-ui-v1";

const COPY = {
  eyebrow: "Meetings",
  defaultTitle: "Product sync",
  meta: "Names in this meeting only.",
  unnamedDesc: "No name typed for this meeting.",
  renamedDesc: "Renamed in this meeting.",
  circle: "○",
  unnamedBadge: "Unnamed",
  ariaPrefix: "Name for",
  legal: "Speaker labels are worked out per meeting and nothing about anyone's voice is saved.",
};

const DEFAULT_SEGMENTS = [
  { id: 1, time: "00:41", slot: "Speaker 1", text: "Homepage first, then the empty state." },
  { id: 2, time: "04:22", slot: "Speaker 2", text: "Export should be one Markdown file." },
  { id: 3, time: "09:48", slot: "Speaker 2", text: "I will send the empty-state copy by Thursday." },
  { id: 4, time: "12:40", slot: "Speaker 2", text: "I will list the open questions too." },
];

function meetingsEnabled(settings: unknown): boolean {
  return (
    typeof settings === "object" &&
    settings !== null &&
    "meetings_enabled" in settings &&
    settings.meetings_enabled === true
  );
}

function macosAtLeast14_4(os: string, osVersion: string): boolean {
  if (os !== "macos") return false;
  const [majorRaw, minorRaw] = osVersion.split(".");
  const major = Number.parseInt(majorRaw ?? "", 10);
  if (!Number.isFinite(major)) return false;
  const minor = minorRaw === undefined ? 0 : Number.parseInt(minorRaw, 10);
  if (!Number.isFinite(minor)) return false;
  return major > 14 || (major === 14 && minor >= 4);
}

export function sidebarVisible(settings: unknown): boolean {
  return meetingsEnabled(settings) && macosAtLeast14_4(platform(), version());
}

export function MeetingsView() {
  const { t } = useTranslation();
  const [names, setNames] = useState<Record<string, string>>({ "Speaker 1": "", "Speaker 2": "Dana" });

  return (
    <div className="mn max-w-3xl w-full mx-auto" data-testid="frame-8-speakers">
      <p className="eyebrow">{t("meetings.sidebar", COPY.eyebrow)}</p>
      <h2 className="meet-title">{COPY.defaultTitle}</h2>
      <p className="meta">{t("meetings.speakersMeta", COPY.meta)}</p>
      <div className="rows">
        {(["Speaker 1", "Speaker 2"] as const).map((slot) => {
          const val = (names[slot] ?? "").trim();
          return (
            <div className="row" key={slot}>
              <div className="row-x">
                <div>
                  <div className="l1"><span className="t">{slot}</span></div>
                  <p className="row-desc">
                    {val ? t("meetings.speakersRenamedDesc", COPY.renamedDesc) : t("meetings.speakersUnnamedDesc", COPY.unnamedDesc)}
                  </p>
                </div>
                {val ? (
                  <input
                    className="field rename"
                    type="text"
                    value={names[slot] ?? ""}
                    aria-label={`${t("meetings.speakersAriaPrefix", COPY.ariaPrefix)} ${slot}`}
                    onChange={(e) => setNames((prev) => ({ ...prev, [slot]: e.target.value }))}
                  />
                ) : (
                  <span className="mark mark-nobody">
                    <span className="sh" aria-hidden="true">{COPY.circle}</span> {t("meetings.speakersUnnamedBadge", COPY.unnamedBadge)}
                  </span>
                )}
              </div>
            </div>
          );
        })}
      </div>
      <div className="transcript" style={{ height: "auto" }}>
        {DEFAULT_SEGMENTS.map((seg) => (
          <div className="tline" key={seg.id}>
            <span className="t-time">{seg.time}</span>
            <span className="t-who">{(names[seg.slot] ?? "").trim() || seg.slot}</span>
            <span>{seg.text}</span>
          </div>
        ))}
      </div>
      <p className="legal">{t("meetings.speakersLegal", COPY.legal)}</p>
    </div>
  );
}

export const meetingsUi = {
  MEETINGS_UI_SENTINEL,
  MeetingsToggle,
  MeetingsView,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
