import { useTranslation } from "react-i18next";
import CocoVoiceTextLogo from "./icons/CocoVoiceTextLogo";
import { useSettings } from "../hooks/useSettings";
import {
  SHELL_ROUTES,
  type ShellGroup,
  type ShellSectionId,
} from "./layout/shellNavigation";

export type SidebarSection = ShellSectionId;

export function Sidebar({
  activeSection,
  onSectionChange,
  showMeetings = false,
}: {
  activeSection: SidebarSection;
  onSectionChange: (section: SidebarSection) => void;
  showMeetings?: boolean;
}) {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const button = (id: SidebarSection, labelKey: string, nested = false) => (
    <button
      key={id}
      type="button"
      data-nav-id={id}
      aria-current={activeSection === id ? "page" : undefined}
      onClick={() => onSectionChange(id)}
      className={`mx-2.5 my-0.5 min-h-8 rounded-full py-1.5 pe-3 text-start text-sm cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary ${nested ? "ps-6" : "ps-3"} ${activeSection === id ? "bg-logo-primary/80 font-semibold" : "hover:bg-mid-gray/15"}`}
    >
      <span className="truncate">{t(labelKey)}</span>
    </button>
  );
  const group = (name: ShellGroup) =>
    SHELL_ROUTES.filter((route) => route.group === name).map((route) =>
      button(route.id, route.labelKey, name === "meetings" || name === "voice"),
    );
  const headingClass =
    "px-4 pt-4 pb-1 text-xs font-semibold uppercase tracking-wide text-mid-gray";
  return (
    <aside className="flex w-[200px] shrink-0 flex-col h-full overflow-y-auto border-e border-mid-gray/20 py-3">
      <CocoVoiceTextLogo width={120} className="m-4" />
      <nav aria-label={t("sidebar.navigation")} className="flex flex-col">
        {/* eslint-disable-next-line i18next/no-literal-string */}
        {group("top")}
        {showMeetings && (
          <>
            <h2 data-nav-heading="meetings" className={headingClass}>
              {t("meetings.sidebar")}
            </h2>
            {/* eslint-disable-next-line i18next/no-literal-string */}
            {group("meetings")}
          </>
        )}
        <h2 data-nav-heading="voice" className={headingClass}>
          {t("sidebar.voiceSection")}
        </h2>
        {/* eslint-disable-next-line i18next/no-literal-string */}
        {group("voice")}
        <div
          data-nav-group="shared"
          className="mt-4 flex flex-col border-t border-mid-gray/20 pt-3"
        >
          {/* eslint-disable-next-line i18next/no-literal-string */}
          {group("shared")}
          {/* eslint-disable-next-line i18next/no-literal-string */}
          {settings?.debug_mode ? button("debug", "sidebar.debug") : null}
        </div>
      </nav>
    </aside>
  );
}
