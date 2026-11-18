import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { emit, listen } from "@tauri-apps/api/event";
import { Users } from "lucide-react";
import { platform, version } from "@tauri-apps/plugin-os";
import { commands, type MeetingOffer } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import { Button } from "../ui/Button";
import Badge from "../ui/Badge";
import { MeetingsToggle } from "../settings/MeetingsToggle";
import type { MeetingsUi } from "./gate";
import { MeetingsView } from "./MeetingsView";

export const MEETINGS_UI_SENTINEL = "coco-meetings-ui-v1";

function meetingsEnabled(settings: unknown): boolean {
  return (
    typeof settings === "object" &&
    settings !== null &&
    "meetings_enabled" in settings &&
    settings.meetings_enabled === true
  );
}

// Same major.minor rule as `macos_at_least_14_4` in src-tauri/src/settings.rs.
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

// Mock 2. The dialog only asks; Start is a separate command and Cancel stores
// nothing. "I've told everyone" is recommended, not a gate, and is never
// remembered: every open starts with it unchecked.
function ConsentDialogPanel({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [toldEveryone, setToldEveryone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    const result = await commands.confirmMeetingStart();
    if (result.status === "error") {
      setError(t("meetings.consent.cannotStart"));
      return;
    }
    setError(null);
    onClose();
  };

  const cancel = async () => {
    await commands.cancelMeetingConsent();
    onClose();
  };

  const copyAnnouncement = async () => {
    try {
      await navigator.clipboard.writeText(t("meetings.consent.announcement"));
    } catch (copyError) {
      console.error("Could not copy the meeting announcement", copyError);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="meetings-consent-reminder"
    >
      <div className="w-full max-w-lg rounded-2xl bg-white p-6 shadow-xl dark:bg-neutral-900">
        <p
          id="meetings-consent-reminder"
          className="text-sm leading-relaxed text-neutral-800 dark:text-neutral-200"
        >
          {t("meetings.consent.reminder")}
        </p>
        <button
          type="button"
          className="mt-4 rounded-lg border border-neutral-300 px-3 py-1.5 text-sm font-medium hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          onClick={() => void copyAnnouncement()}
        >
          {t("meetings.consent.copyAnnouncement")}
        </button>
        <p className="mt-3 text-sm text-neutral-600 dark:text-neutral-400">
          {t("meetings.consent.orgRules")}
        </p>
        <label className="mt-4 flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={toldEveryone}
            onChange={(event) => setToldEveryone(event.target.checked)}
          />
          {t("meetings.consent.toldEveryone")}
        </label>
        {error && (
          <p className="mt-3 text-sm text-red-600" role="alert">
            {error}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            className="rounded-lg px-3 py-1.5 text-sm font-medium hover:bg-neutral-100 dark:hover:bg-neutral-800"
            onClick={() => void cancel()}
          >
            {t("meetings.consent.cancel")}
          </button>
          <button
            type="button"
            className="rounded-lg bg-neutral-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-neutral-800 dark:bg-white dark:text-neutral-900"
            onClick={() => void start()}
          >
            {t("meetings.consent.start")}
          </button>
        </div>
      </div>
    </div>
  );
}

// Mounted app-wide (App.tsx) so the shortcut and the offer open the dialog from
// any section, not only while the Meetings view is on screen.
export function ConsentDialog() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("meetings-open-consent", () => setOpen(true)).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!open) return null;
  return <ConsentDialogPanel onClose={() => setOpen(false)} />;
}

function sameOffer(a: MeetingOffer, b: MeetingOffer): boolean {
  return a.bundleId === b.bundleId && a.app === b.app && a.inCall === b.inCall;
}

export function OfferPanel() {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const titleId = useId();
  const bodyId = useId();
  const [offer, setOffer] = useState<MeetingOffer | null>(null);
  const [held, setHeld] = useState(false);
  const sawEvent = useRef(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const apply = (next: MeetingOffer | null) => {
      if (next == null) {
        setOffer(null);
        setHeld(false);
        return;
      }
      setOffer((prev) => (prev && sameOffer(prev, next) ? prev : next));
    };
    listen<MeetingOffer | null>("meetings-offer", (event) => {
      sawEvent.current = true;
      apply(event.payload);
    }).then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    });
    commands
      .currentMeetingOffer()
      .then((result) => {
        if (cancelled || sawEvent.current || result.status === "error") return;
        apply(result.data);
      })
      .catch((error: unknown) => {
        console.error("Failed to read the meeting offer", error);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!sidebarVisible(settings) || !offer || held) return null;

  const onStart = () => {
    setHeld(true);
    void emit("meetings-open-consent", {
      bundleId: offer.bundleId,
      app: offer.app,
    });
  };

  const onNotNow = () => {
    const bundleId = offer.bundleId;
    sawEvent.current = true;
    setOffer(null);
    void commands.dismissMeetingOffer(bundleId).then(
      (result) => {
        if (result.status === "error") {
          console.error("Failed to dismiss the meeting offer", result.error);
        }
      },
      (error: unknown) => {
        console.error("Failed to dismiss the meeting offer", error);
      },
    );
  };

  return (
    <div
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-describedby={bodyId}
      className="fixed bottom-16 left-1/2 z-40 w-[min(100%-2rem,24rem)] -translate-x-1/2 rounded-lg border border-mid-gray/20 bg-background p-4 shadow-xl"
    >
      <h2 id={titleId} className="text-base font-semibold text-text">
        {t("meetings.offer.title")}
      </h2>
      <p id={bodyId} className="mt-1 text-sm text-text">
        {offer.app === "meet"
          ? t("meetings.offer.meet")
          : offer.app === "teams"
            ? t(
                offer.inCall
                  ? "meetings.offer.teamsInCall"
                  : "meetings.offer.teamsOpen",
              )
            : t(
                offer.inCall
                  ? "meetings.offer.zoomInCall"
                  : "meetings.offer.zoomOpen",
              )}
      </p>
      <div className="mt-3">
        <Badge variant="secondary">{t("meetings.offer.notRecording")}</Badge>
      </div>
      <p className="mt-2 text-xs text-mid-gray">
        {t("meetings.offer.neverOnItsOwn")}
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button type="button" variant="primary" onClick={onStart}>
          {t("meetings.offer.start")}
        </Button>
        <Button type="button" variant="secondary" onClick={onNotNow}>
          {t("meetings.offer.notNow")}
        </Button>
      </div>
    </div>
  );
}

export const meetingsUi = {
  MEETINGS_UI_SENTINEL,
  MeetingsToggle,
  ConsentDialog,
  MeetingsView,
  OfferPanel,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
