import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Users } from "lucide-react";
import { platform, version } from "@tauri-apps/plugin-os";
import { MeetingsToggle } from "../settings/MeetingsToggle";
import { commands } from "../../bindings.meetings";
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

export function MeetingsView() {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto">
      <p className="text-sm font-medium">{t("meetings.sidebar")}</p>
    </div>
  );
}

export const meetingsUi = {
  MEETINGS_UI_SENTINEL,
  MeetingsToggle,
  ConsentDialog,
  MeetingsView,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
