import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands as meetingCommands,
  type MeetingSegmentView,
  type MeetingSpeakerSlot,
} from "../../bindings.meetings";

interface SpeakerSplitSectionProps {
  meetingId: number;
  segments: MeetingSegmentView[];
  initialSlots?: MeetingSpeakerSlot[];
  working?: boolean;
  modelsMissing?: boolean;
  onDownloadModels?: () => void;
  onSlotsLoaded?: (slots: MeetingSpeakerSlot[]) => void;
  onRenamed?: (slots: MeetingSpeakerSlot[]) => void;
}

const UNNAMED_GLYPH = "\u25CB";

export function slotDisplayName(row: MeetingSpeakerSlot): string {
  return (row.custom_name || row.display_name || "").trim();
}

export function buildSpeakerNameMap(
  slots: MeetingSpeakerSlot[],
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of slots) {
    const name = slotDisplayName(row);
    if (name) {
      out[row.slot] = name;
    }
  }
  return out;
}

function deriveFallbackSlots(
  segments: MeetingSegmentView[],
): MeetingSpeakerSlot[] {
  const seen = new Set<string>();
  const out: MeetingSpeakerSlot[] = [];
  for (const seg of segments) {
    const raw = (seg.speaker_label ?? "").trim();
    if (raw.startsWith("Speaker ") && !seen.has(raw)) {
      seen.add(raw);
      out.push({ slot: raw, custom_name: "" });
    }
  }
  return out.sort((a, b) => a.slot.localeCompare(b.slot));
}

export const SpeakerSplitSection: React.FC<SpeakerSplitSectionProps> = ({
  meetingId,
  segments,
  initialSlots,
  working = false,
  modelsMissing = false,
  onDownloadModels,
  onSlotsLoaded,
  onRenamed,
}) => {
  const { t } = useTranslation();
  const [slots, setSlots] = useState<MeetingSpeakerSlot[]>(
    () => initialSlots ?? deriveFallbackSlots(segments),
  );
  const [editingSlot, setEditingSlot] = useState<string | null>(null);
  const onSlotsLoadedRef = useRef(onSlotsLoaded);
  onSlotsLoadedRef.current = onSlotsLoaded;

  useEffect(() => {
    if (initialSlots) {
      setSlots(initialSlots);
      onSlotsLoadedRef.current?.(initialSlots);
      return;
    }
    let cancelled = false;
    void meetingCommands.listMeetingSpeakers(meetingId).then((res) => {
      if (cancelled) return;
      const next =
        res.status === "ok" && res.data.length > 0
          ? res.data.map((row) => ({
              ...row,
              custom_name: slotDisplayName(row),
            }))
          : deriveFallbackSlots(segments);
      setSlots(next);
      onSlotsLoadedRef.current?.(next);
    });
    return () => {
      cancelled = true;
    };
  }, [meetingId, segments, initialSlots]);

  const handleRename = (slot: string, nextValue: string) => {
    setSlots((prev) =>
      prev.map((row) =>
        row.slot === slot
          ? { ...row, custom_name: nextValue, display_name: nextValue }
          : row,
      ),
    );
  };

  const commitRename = (slot: string, nextValue: string) => {
    void meetingCommands
      .renameMeetingSpeakerSlot(meetingId, slot, nextValue)
      .then((res) => {
        if (res.status === "ok") {
          const normalized = res.data.map((row) => ({
            ...row,
            custom_name: slotDisplayName(row),
          }));
          setSlots(normalized);
          onSlotsLoaded?.(normalized);
          onRenamed?.(normalized);
        }
      });
  };

  return (
    <section
      data-testid="frame-8-speakers"
      className="rounded-[12px] border border-mid-gray/25 bg-surface p-3.5 space-y-2.5"
    >
      <div className="space-y-0.5">
        <h3 className="text-[15px] font-semibold text-text">
          {t("meetings.speakersTitle", "Speakers")}
        </h3>
        <p className="text-[12.5px] text-mid-gray">
          {t("meetings.speakersMeta", "Names in this meeting only.")}
        </p>
      </div>

      {working ? (
        <p className="text-[12.5px] text-mid-gray" role="status">
          {t("meetings.speakersWorking", "Working out speakers…")}
        </p>
      ) : modelsMissing ? (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-[10px] border border-mid-gray/20 px-3 py-2 text-[12.5px] text-mid-gray">
          <span>
            {t(
              "meetings.speakersDownloadNeeded",
              "Speaker split needs a 31 MB download, made once.",
            )}
          </span>
          {onDownloadModels ? (
            <button
              type="button"
              onClick={onDownloadModels}
              className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("meetings.speakersDownloadButton", "Download")}
            </button>
          ) : null}
        </div>
      ) : slots.length === 0 ? (
        <p className="text-[12.5px] text-mid-gray">
          {t(
            "meetings.speakersNoneYet",
            "No separated speaker turns in Others for this meeting.",
          )}
        </p>
      ) : (
        <div className="divide-y divide-mid-gray/15 rounded-[10px] border border-mid-gray/20">
          {slots.map((row) => {
            const resolvedName = slotDisplayName(row);
            const hasName = resolvedName.length > 0;
            const showInput = hasName || editingSlot === row.slot;
            return (
              <div
                key={row.slot}
                className="flex items-center justify-between gap-3 px-3.5 py-2.5"
              >
                <div className="min-w-0 space-y-0.5">
                  <div className="text-sm font-medium text-text">{row.slot}</div>
                  <p className="text-xs text-mid-gray">
                    {hasName
                      ? t(
                          "meetings.speakersRenamedDesc",
                          "Renamed in this meeting.",
                        )
                      : t(
                          "meetings.speakersUnnamedDesc",
                          "No name typed for this meeting.",
                        )}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {!hasName && !showInput ? (
                    <button
                      type="button"
                      onClick={() => setEditingSlot(row.slot)}
                      className="inline-flex items-center gap-1 rounded-full border border-mid-gray/35 px-2.5 py-0.5 text-[11.5px] font-medium text-mid-gray hover:text-text cursor-pointer"
                    >
                      <span aria-hidden="true">{UNNAMED_GLYPH}</span>
                      <span>
                        {t("meetings.speakersUnnamedBadge", "Unnamed")}
                      </span>
                    </button>
                  ) : (
                    <input
                      type="text"
                      value={resolvedName}
                      placeholder={row.slot}
                      aria-label={`${t("meetings.speakersAriaPrefix", "Name for")} ${row.slot}`}
                      onChange={(e) => handleRename(row.slot, e.target.value)}
                      onBlur={(e) => {
                        if (!e.target.value.trim()) setEditingSlot(null);
                        commitRename(row.slot, e.target.value);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          commitRename(row.slot, resolvedName);
                        }
                      }}
                      className="w-40 rounded-lg border border-mid-gray/30 bg-background px-2.5 py-1 text-sm text-text"
                    />
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      <p className="text-[11.5px] text-mid-gray">
        {t(
          "meetings.speakersLegal",
          "Speaker labels are worked out per meeting and nothing about anyone's voice is saved.",
        )}
      </p>
    </section>
  );
};
