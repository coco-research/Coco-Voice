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

export function MeetingsView() {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto">
      <p className="text-sm font-medium">{t("meetings.sidebar")}</p>
    </div>
  );
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
  MeetingsView,
  OfferPanel,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
