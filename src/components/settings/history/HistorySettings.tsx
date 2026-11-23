import React, { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { useTranslation } from "react-i18next";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
  type KeepDictationsChoice,
  type RecordingRetentionPeriod,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { OUTLINED_BTN_SM } from "../../meetings/MeetingsList";
import { Row, Segmented } from "../../ui";

const PAGE_SIZE = 30;
const QUOTE = '"';
const MID_SEP = " · ";
const TIME_OPTS: Intl.DateTimeFormatOptions = {
  hour: "numeric",
  minute: "2-digit",
  hour12: false,
};
const SHORT_DATE_OPTS: Intl.DateTimeFormatOptions = {
  day: "numeric",
  month: "short",
};

const displayTextFor = (e: HistoryEntry): string =>
  e.post_processed_text && e.post_processed_text.trim().length > 0
    ? e.post_processed_text
    : e.transcription_text;

function sameCalendarDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function capFirst(value: string): string {
  return value ? value.charAt(0).toUpperCase() + value.slice(1) : value;
}

export function formatDictationTime(
  timestampSec: number,
  lang: string,
  nowSec: number = Math.floor(Date.now() / 1000),
): string {
  const date = new Date(timestampSec * 1000);
  const now = new Date(nowSec * 1000);
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);

  const rawClock = date.toLocaleTimeString(lang, TIME_OPTS);
  const clock = rawClock.replace(/^0(?=\d:)/, "");
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });

  if (sameCalendarDay(date, now)) {
    return `${capFirst(rtf.format(0, "day"))} ${clock}`;
  }
  if (sameCalendarDay(date, yesterday)) {
    return `${capFirst(rtf.format(-1, "day"))} ${clock}`;
  }
  return `${date.toLocaleDateString(lang, SHORT_DATE_OPTS)} ${clock}`;
}

function isDictationEntry(x: unknown): x is HistoryEntry {
  const o =
    typeof x === "object" && x !== null ? (x as Record<string, unknown>) : null;
  return Boolean(
    o &&
    typeof o.transcription_text === "string" &&
    !("notes_ready" in o) &&
    !("started_at" in o),
  );
}

export function selectDictationEntries(
  records: readonly unknown[],
  query: string,
): HistoryEntry[] {
  const q = query.trim().toLowerCase();
  const takes = records.filter(isDictationEntry);
  return q
    ? takes.filter((e) =>
        `${displayTextFor(e)} ${e.app_name ?? ""} ${e.title}`
          .toLowerCase()
          .includes(q),
      )
    : takes;
}

export type KeepDictationsButtonChoice =
  "never" | "hours_24" | "days_30" | "forever";

export type EffectiveKeepChoice =
  | KeepDictationsButtonChoice
  | "preserve_limit"
  | "days_3"
  | "weeks_2"
  | "months_3";

export function resolveKeepDictationsChoice(
  neverStoreHistory: boolean,
  retentionPeriod: RecordingRetentionPeriod | string | undefined,
): EffectiveKeepChoice {
  if (neverStoreHistory) return "never";
  switch (retentionPeriod) {
    case "hours_24":
    case "hours24":
      return "hours_24";
    case "days_30":
    case "days30":
    case undefined:
      return "days_30";
    case "never":
    case "forever":
      return "forever";
    case "preserve_limit":
      return "preserve_limit";
    case "days_3":
    case "days3":
      return "days_3";
    case "weeks_2":
    case "weeks2":
      return "weeks_2";
    case "months_3":
    case "months3":
      return "months_3";
    default:
      return "days_30";
  }
}

export function isDrawnKeepChoice(
  choice: EffectiveKeepChoice,
): choice is KeepDictationsButtonChoice {
  return (
    choice === "never" ||
    choice === "hours_24" ||
    choice === "days_30" ||
    choice === "forever"
  );
}

export interface HistoryProps {
  initialEntries?: readonly unknown[];
  initialQuery?: string;
  retentionText?: string;
  nowTimestamp?: number;
  initialNeverStore?: boolean;
  initialRetentionPeriod?: RecordingRetentionPeriod;
  initialKeepAudio?: boolean;
  initialHistoryLimit?: number;
}

export const HistorySettings: React.FC<HistoryProps> = (props) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const { settings, updateSetting, refreshSettings } = useSettings();
  const neverStore = Boolean(
    props.initialNeverStore ?? settings?.never_store_history ?? false,
  );
  const retentionPeriod =
    props.initialRetentionPeriod ??
    settings?.recording_retention_period ??
    "days_30";
  const historyLimit =
    props.initialHistoryLimit ?? settings?.history_limit ?? 5;
  const keepChoice = resolveKeepDictationsChoice(neverStore, retentionPeriod);
  const keepAudio = Boolean(
    props.initialKeepAudio ?? settings?.keep_dictation_audio ?? false,
  );
  const [entries, setEntries] = useState<HistoryEntry[]>(() =>
    props.initialEntries
      ? selectDictationEntries(props.initialEntries, "")
      : [],
  );
  const [loading, setLoading] = useState(!props.initialEntries);
  const [query, setQuery] = useState(props.initialQuery ?? "");
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<HistoryEntry[]>([]);
  const loadingRef = useRef(false);

  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const loadPage = useCallback(async (cursor?: number) => {
    const isFirstPage = cursor === undefined;
    if (!isFirstPage && loadingRef.current) return;
    loadingRef.current = true;
    if (isFirstPage) setLoading(true);
    try {
      const result = await commands.getHistoryEntries(
        cursor ?? null,
        PAGE_SIZE,
      );
      if (result.status === "ok") {
        const { entries: newEntries, has_more } = result.data;
        setEntries((prev) =>
          isFirstPage ? newEntries : [...prev, ...newEntries],
        );
        setHasMore(has_more);
      }
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
      loadingRef.current = false;
    }
  }, []);

  useEffect(() => {
    if (props.initialEntries) return;
    void loadPage();
  }, [props.initialEntries, loadPage]);

  useEffect(() => {
    if (loading || props.initialEntries) return;
    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;
    const observer = new IntersectionObserver(
      (observerEntries) => {
        if (observerEntries[0]?.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) void loadPage(lastEntry.id);
        }
      },
      { threshold: 0 },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage, props.initialEntries]);

  useEffect(() => {
    if (props.initialEntries) return;
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        setEntries((prev) => [payload.entry, ...prev]);
      } else if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [props.initialEntries]);

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  const getAudioUrl = useCallback(
    async (fileName: string) => {
      try {
        const result = await commands.getAudioFilePath(fileName);
        if (result.status === "ok") {
          if (osType === "linux") {
            const fileData = await readFile(result.data);
            const blob = new Blob([fileData], { type: "audio/wav" });
            return URL.createObjectURL(blob);
          }
          return convertFileSrc(result.data, "asset");
        }
        return null;
      } catch (error) {
        console.error("Failed to get audio file path:", error);
        return null;
      }
    },
    [osType],
  );

  const filtered = selectDictationEntries(entries, query);
  const preserveLimitLabel = t(
    "settings.debug.recordingRetention.preserveLimit",
    { count: Number(historyLimit) },
  );
  const choiceLabelMap: Record<EffectiveKeepChoice, string> = {
    never: t("settings.history.optionNever", "Never"),
    hours_24: t("settings.history.option24Hours", "24 hours"),
    days_30: t("settings.history.option30Days", "30 days"),
    forever: t("settings.history.optionForever", "Forever"),
    preserve_limit: preserveLimitLabel,
    days_3: t("settings.debug.recordingRetention.days3"),
    weeks_2: t("settings.debug.recordingRetention.weeks2"),
    months_3: t("settings.debug.recordingRetention.months3"),
  };
  const headerBadgeMap: Record<EffectiveKeepChoice, string | null> = {
    never: null,
    hours_24: t("settings.history.kept24HoursBadge", "Kept 24 hours"),
    days_30: t("sidebar.keptDays"),
    forever: t("settings.history.keptForeverBadge", "Kept forever"),
    preserve_limit: preserveLimitLabel,
    days_3: t("settings.debug.recordingRetention.days3"),
    weeks_2: t("settings.debug.recordingRetention.weeks2"),
    months_3: t("settings.debug.recordingRetention.months3"),
  };
  const retentionLabel =
    props.retentionText !== undefined
      ? props.retentionText
      : headerBadgeMap[keepChoice];

  const applyChoice = async (payload: KeepDictationsChoice) => {
    const result = await commands.updateKeepDictations(payload);
    if (result.status === "ok") {
      await refreshSettings();
      if (!props.initialEntries) {
        await loadPage();
      }
    } else {
      await refreshSettings();
    }
  };

  const handleSelectKeepChoice = async (choice: KeepDictationsButtonChoice) => {
    const payload: KeepDictationsChoice =
      choice === "never"
        ? { kind: "never" }
        : choice === "hours_24"
          ? { kind: "hours24" }
          : choice === "forever"
            ? { kind: "forever" }
            : { kind: "days30" };
    await applyChoice(payload);
  };

  const handleSelectLegacyChoice = async (raw: string) => {
    if (raw === "preserve_limit") {
      await applyChoice({ kind: "keep_latest", count: Number(historyLimit) });
    } else if (raw === "days_3") {
      await applyChoice({ kind: "days3" });
    } else if (raw === "weeks_2") {
      await applyChoice({ kind: "weeks2" });
    } else if (raw === "months_3") {
      await applyChoice({ kind: "months3" });
    }
  };
  let content: React.ReactNode;

  if (loading) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("settings.history.loading")}
      </p>
    );
  } else if (entries.length === 0) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("settings.history.empty")}
      </p>
    );
  } else if (filtered.length === 0) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("sidebar.emptyDictationSearch")}
      </p>
    );
  } else {
    content = (
      <>
        <div className="mt-3 divide-y divide-mid-gray/20 border-b-[1.5px] border-text/80">
          {filtered.map((entry) => (
            <HistoryRow
              key={entry.id}
              entry={entry}
              nowTimestamp={props.nowTimestamp}
              onCopyText={() => void copyToClipboard(displayTextFor(entry))}
              getAudioUrl={getAudioUrl}
            />
          ))}
        </div>
        <div ref={sentinelRef} className="h-1" />
      </>
    );
  }

  return (
    <div
      data-testid="dictation-history-section"
      className="max-w-3xl w-full mx-auto"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("sidebar.voiceSection")}
      </p>
      <header className="mt-1 flex items-baseline justify-between gap-3">
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.dictationHistory")}
        </h2>
        {retentionLabel ? (
          <span className="text-xs font-medium text-mid-gray">
            {retentionLabel}
          </span>
        ) : null}
      </header>
      <section
        data-testid="dictation-privacy-controls"
        className="mt-3 rounded-[14px] border border-mid-gray/25 bg-surface px-4 py-3 space-y-2.5"
      >
        <h3 className="text-[14px] font-semibold text-text">
          {t("settings.history.keepDictationsHeading", "Keep dictations")}
        </h3>
        <div className="divide-y divide-mid-gray/15">
          <Row
            label={t("settings.history.keepLabel", "Keep")}
            subLabel={choiceLabelMap[keepChoice]}
            className="pt-0"
            action={
              <Segmented<KeepDictationsButtonChoice>
                ariaLabel={t(
                  "settings.history.keepDictationsHeading",
                  "Keep dictations",
                )}
                options={[
                  { value: "never", label: choiceLabelMap.never },
                  { value: "hours_24", label: choiceLabelMap.hours_24 },
                  { value: "days_30", label: choiceLabelMap.days_30 },
                  { value: "forever", label: choiceLabelMap.forever },
                ]}
                value={
                  isDrawnKeepChoice(keepChoice)
                    ? keepChoice
                    : ("" as KeepDictationsButtonChoice)
                }
                onChange={(opt) => void handleSelectKeepChoice(opt)}
              />
            }
          />

          <Row
            label={t("settings.history.keepAudioTitle", "Keep the audio")}
            subLabel={
              keepAudio
                ? t(
                    "settings.history.keepAudioOnDesc",
                    "On: audio is saved alongside text",
                  )
                : t(
                    "settings.history.keepAudioOffDesc",
                    "Off: only the text is kept",
                  )
            }
            action={
              <button
                type="button"
                role="switch"
                aria-checked={keepAudio}
                aria-label={t(
                  "settings.history.keepAudioTitle",
                  "Keep the audio",
                )}
                onClick={() =>
                  void updateSetting("keep_dictation_audio", !keepAudio)
                }
                className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full border transition-colors cursor-pointer ${
                  keepAudio
                    ? "border-logo-primary bg-logo-primary"
                    : "border-mid-gray/50 bg-transparent"
                }`}
              >
                <span
                  className={`inline-block h-4 w-4 rounded-full transition-transform ${
                    keepAudio
                      ? "translate-x-[18px] bg-white"
                      : "translate-x-[2px] bg-text"
                  }`}
                />
              </button>
            }
          />

          <div className="flex flex-wrap items-center justify-between gap-3 pt-2.5">
            <div className="text-[12.5px] font-medium text-mid-gray">
              {t("settings.debug.recordingRetention.title")}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label={t("settings.debug.recordingRetention.title")}
                value={isDrawnKeepChoice(keepChoice) ? "" : keepChoice}
                onChange={(e) => void handleSelectLegacyChoice(e.target.value)}
                className={`rounded-[8px] border bg-background px-2 py-1 text-[12px] transition-opacity ${
                  isDrawnKeepChoice(keepChoice)
                    ? "border-mid-gray/25 text-mid-gray opacity-60 hover:opacity-100 focus:opacity-100"
                    : "border-mid-gray/40 text-text"
                }`}
              >
                <option value="">
                  {isDrawnKeepChoice(keepChoice)
                    ? ""
                    : t("settings.debug.recordingRetention.placeholder")}
                </option>
                <option value="preserve_limit">{preserveLimitLabel}</option>
                <option value="days_3">
                  {t("settings.debug.recordingRetention.days3")}
                </option>
                <option value="weeks_2">
                  {t("settings.debug.recordingRetention.weeks2")}
                </option>
                <option value="months_3">
                  {t("settings.debug.recordingRetention.months3")}
                </option>
              </select>
              {keepChoice === "preserve_limit" ? (
                <label className="inline-flex items-center gap-1.5 text-[12px] text-mid-gray">
                  <input
                    type="number"
                    min={0}
                    max={1000}
                    value={historyLimit}
                    aria-label={t("settings.debug.historyLimit.title")}
                    onChange={(e) => {
                      const next = Number.parseInt(e.target.value, 10);
                      if (!Number.isNaN(next) && next >= 0) {
                        void applyChoice({ kind: "keep_latest", count: next });
                      }
                    }}
                    className="w-16 rounded-[8px] border border-mid-gray/40 bg-background px-2 py-0.5 text-[12px] text-text"
                  />
                  <span>{t("settings.debug.historyLimit.entries")}</span>
                </label>
              ) : null}
            </div>
          </div>
        </div>
      </section>
      <p className="mt-2 text-[12px] text-mid-gray">
        {t(
          "settings.history.neverFooter",
          "Never: no text and no audio is written to disk. History stays empty.",
        )}
      </p>
      <label className="mt-3 block">
        <span className="block text-xs font-medium text-mid-gray">
          {t("meetings.list.searchLabel")}
        </span>
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("sidebar.dictationSearchPlaceholder")}
          aria-label={t("sidebar.dictationSearchAria")}
          className="mt-1 w-full rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text focus:outline-none focus:ring-2 focus:ring-logo-primary"
        />
      </label>
      {content}
    </div>
  );
};

interface HistoryRowProps {
  entry: HistoryEntry;
  nowTimestamp?: number;
  onCopyText: () => void;
  getAudioUrl: (fileName: string) => Promise<string | null>;
}

const HistoryRow: React.FC<HistoryRowProps> = ({
  entry,
  nowTimestamp,
  onCopyText,
  getAudioUrl,
}) => {
  const { t, i18n } = useTranslation();
  const [playing, setPlaying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    return () => {
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
    };
  }, []);

  const hasTranscription = entry.transcription_text.trim().length > 0;
  const lineOne = hasTranscription
    ? `${QUOTE}${displayTextFor(entry)}${QUOTE}`
    : t("settings.history.transcriptionFailed");
  const appName = entry.app_name?.trim() || "";
  const timeLabel = formatDictationTime(
    entry.timestamp,
    i18n.language,
    nowTimestamp,
  );
  const isCleaned = Boolean(
    entry.post_processed_text && entry.post_processed_text.trim().length > 0,
  );
  const metaTail = isCleaned
    ? `${timeLabel}${MID_SEP}${t("sidebar.cleanedUp")}`
    : timeLabel;
  const playLabel = playing ? t("common.pause") : t("common.play");

  const handlePlay = async () => {
    if (playing && audioRef.current) {
      audioRef.current.pause();
      setPlaying(false);
      return;
    }
    const url = await getAudioUrl(entry.file_name);
    if (!url) return;
    const audio = new Audio(url);
    audioRef.current = audio;
    audio.onended = () => setPlaying(false);
    audio.onerror = () => setPlaying(false);
    setPlaying(true);
    void audio.play().catch(() => setPlaying(false));
  };

  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <p className="text-[14px] font-medium text-text break-words">
          {lineOne}
        </p>
        <p className="mt-0.5 text-[13px] text-mid-gray">
          {appName ? (
            <>
              <span className="font-mono text-[12px] font-semibold text-text">
                {appName}
              </span>
              {MID_SEP}
              {metaTail}
            </>
          ) : (
            metaTail
          )}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={onCopyText}
          disabled={!hasTranscription}
          className={OUTLINED_BTN_SM}
        >
          {t("common.copy")}
        </button>
        {entry.file_name.trim().length > 0 ? (
          <button
            type="button"
            onClick={() => void handlePlay()}
            className={OUTLINED_BTN_SM}
          >
            {playLabel}
          </button>
        ) : null}
      </div>
    </div>
  );
};
