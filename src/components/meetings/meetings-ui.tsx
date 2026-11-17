import { useTranslation } from "react-i18next";
import { Users } from "lucide-react";
import { platform, version } from "@tauri-apps/plugin-os";
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

export const meetingsUi = {
  MEETINGS_UI_SENTINEL,
  MeetingsToggle,
  MeetingsView,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
