import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type MeetingOffer,
  type MeetingSegmentView,
  type MeetingStatus,
} from "../../bindings.meetings";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { LiveTranscriptSection } from "./LiveTranscriptSection";
import { ConsentDialog, OfferPanel } from "./MeetingEntryPanels";

const DOT_FILLED = "●";
const CLOSE_X = "×";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
export const MEETING_WORDS_STORAGE_KEY = "coco_voice.meeting_custom_words";

export function normalizeMeetingWordList(
  existing: string[],
  addition: string,
): string[] {
  const trimmed = addition.trim();
  if (!trimmed) return existing;
  if (existing.some((w) => w.toLowerCase() === trimmed.toLowerCase())) {
    return existing;
  }
  return [...existing, trimmed];
}

export interface RecordMeetingSectionProps {
  offerEnabled: boolean;
  offer: MeetingOffer | null;
  consentOpen: boolean;
  status: MeetingStatus;
  segments: MeetingSegmentView[];
  meetingWords: string[];
  onToggleOffer: (enabled: boolean) => void;
  onOpenConsent: () => void;
  onConfirmStart: () => void;
  onCancelConsent: () => void;
  onDismissOffer: (bundleId: string) => void;
  onStopMeeting: () => void;
  onAddMeetingWord: (word: string) => void;
  onRemoveMeetingWord: (word: string) => void;
}

export const RecordMeetingSection: React.FC<RecordMeetingSectionProps> = ({
  offerEnabled,
  offer,
  consentOpen,
  status,
  segments,
  meetingWords,
  onToggleOffer,
  onOpenConsent,
  onConfirmStart,
  onCancelConsent,
  onDismissOffer,
  onStopMeeting,
  onAddMeetingWord,
  onRemoveMeetingWord,
}) => {
  const { t } = useTranslation();
  const [wordDraft, setWordDraft] = useState("");
  const activeSession =
    status.state === "recording" ||
    status.state === "paused" ||
    segments.length > 0;

  const submitWord = () => {
    const trimmed = wordDraft.trim();
    if (!trimmed) return;
    onAddMeetingWord(trimmed);
    setWordDraft("");
  };

  return (
    <div
      data-testid="page-2-record-meeting"
      className="max-w-3xl w-full mx-auto space-y-3.5"
    >
      <p className={EYEBROW}>{t("meetings.sidebar")}</p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t("sidebar.recordMeeting")}
      </h2>

      {offer ? (
        <OfferPanel
          offer={offer}
          onStartNotes={onOpenConsent}
          onDismiss={onDismissOffer}
        />
      ) : null}

      <ConsentDialog
        open={consentOpen}
        onConfirmStart={onConfirmStart}
        onCancel={onCancelConsent}
      />

      {activeSession ? (
        <LiveTranscriptSection
          status={status}
          segments={segments}
          onStop={onStopMeeting}
        />
      ) : null}

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5">
        <h3 className="text-[15px] font-semibold text-text">
          {t("meetings.record.startNowHeading")}
        </h3>
        <button
          type="button"
          onClick={onOpenConsent}
          className="inline-flex min-h-[34px] items-center gap-1.5 rounded-[10px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          <span aria-hidden="true">{DOT_FILLED}</span>
          <span>{t("sidebar.startRecording")}</span>
        </button>
      </section>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-2">
        <div className="px-2 pt-1.5 text-[13px] font-semibold text-text">
          {t("meetings.record.whenCallStartsHeading")}
        </div>
        <ToggleSwitch
          checked={offerEnabled}
          onChange={onToggleOffer}
          label={t("meetings.record.offerLabel")}
          description={t("meetings.record.offerDescription")}
          descriptionMode="inline"
          grouped={true}
        />
      </section>

      <section
        data-testid="meeting-custom-words"
        className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
      >
        <div>
          <h3 className="text-[15px] font-semibold text-text">
            {t("meetings.record.wordsHeading")}
          </h3>
          <p className="text-[12.5px] text-mid-gray">
            {t("meetings.record.wordsDescription")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="text"
            value={wordDraft}
            onChange={(e) => setWordDraft(e.target.value)}
            placeholder={t("meetings.record.wordsPlaceholder")}
            aria-label={t("meetings.record.wordsHeading")}
            className="min-w-0 flex-1 rounded-[9px] border border-mid-gray/35 bg-background px-3 py-1.5 text-[13px] text-text"
          />
          <button
            type="button"
            onClick={submitWord}
            className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.record.addWord")}
          </button>
        </div>
        {meetingWords.length > 0 ? (
          <div className="flex flex-wrap gap-1.5 pt-0.5">
            {meetingWords.map((word) => (
              <span
                key={word}
                className="inline-flex items-center gap-1 rounded-full border border-mid-gray/35 bg-mid-gray/[0.08] px-2.5 py-0.5 text-[12px] font-medium text-text"
              >
                <span>{word}</span>
                <button
                  type="button"
                  aria-label={t("meetings.record.removeWordAria", { word })}
                  onClick={() => onRemoveMeetingWord(word)}
                  className="text-mid-gray hover:text-text cursor-pointer"
                >
                  {CLOSE_X}
                </button>
              </span>
            ))}
          </div>
        ) : null}
      </section>

      <p className="text-[12px] text-mid-gray">
        {t("meetings.record.dictationKeepsWorking")}
      </p>
    </div>
  );
};

export const RecordMeetingPage: React.FC = () => {
  const [offerEnabled, setOfferEnabled] = useState(true);
  const [offer, setOffer] = useState<MeetingOffer | null>(null);
  const [consentOpen, setConsentOpen] = useState(false);
  const [status, setStatus] = useState<MeetingStatus>({
    state: "idle",
    elapsed_secs: 0,
    meeting_id: null,
    mic_name: null,
    warnings: [],
  });
  const [segments] = useState<MeetingSegmentView[]>([]);
  const [meetingWords, setMeetingWords] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const current = await commands.meetingStatus();
      if (!cancelled) setStatus(current);
      const offerRes = await commands.currentMeetingOffer();
      if (!cancelled && offerRes.status === "ok") setOffer(offerRes.data);
      const wordsRes = await commands.getMeetingCustomWords();
      if (!cancelled && wordsRes.status === "ok") {
        setMeetingWords(wordsRes.data);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const persistWords = (next: string[]) => {
    setMeetingWords(next);
    void commands.setMeetingCustomWords(next);
  };

  return (
    <RecordMeetingSection
      offerEnabled={offerEnabled}
      offer={offer}
      consentOpen={consentOpen || status.state === "consent_open"}
      status={status}
      segments={segments}
      meetingWords={meetingWords}
      onToggleOffer={(enabled) => {
        setOfferEnabled(enabled);
        void commands.changeMeetingsEnabledSetting(enabled);
      }}
      onOpenConsent={() => {
        setConsentOpen(true);
        void commands.openMeetingConsent();
      }}
      onConfirmStart={() => {
        setConsentOpen(false);
        void commands.confirmMeetingStart().then((res) => {
          if (res.status === "ok") setStatus(res.data);
        });
      }}
      onCancelConsent={() => {
        setConsentOpen(false);
        void commands.cancelMeetingConsent();
      }}
      onDismissOffer={(bundleId) => {
        setOffer(null);
        void commands.dismissMeetingOffer(bundleId);
      }}
      onStopMeeting={() => {
        void commands.stopMeeting().then((res) => {
          if (res.status === "ok") setStatus(res.data);
        });
      }}
      onAddMeetingWord={(w) => {
        persistWords(normalizeMeetingWordList(meetingWords, w));
      }}
      onRemoveMeetingWord={(w) => {
        persistWords(meetingWords.filter((item) => item !== w));
      }}
    />
  );
};
