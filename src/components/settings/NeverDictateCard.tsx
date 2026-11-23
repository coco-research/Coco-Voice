import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { OUTLINED_BTN_SM } from "../meetings/MeetingsList";

const INPUT_CLS =
  "flex-1 min-w-[180px] rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text";

export interface BlockedSiteEntry {
  name: string;
  browser_identifier: string;
  domain: string;
}

export interface NeverDictateCardProps {
  blockedApps: string[];
  blockedSites?: BlockedSiteEntry[];
  displayNames?: Record<string, string>;
  onSaveBlockedApps: (next: string[]) => Promise<void>;
  onSaveBlockedSites?: (next: BlockedSiteEntry[]) => Promise<void>;
  onReadFrontApp?: () => Promise<{
    app_name: string;
    bundle_id: string | null;
  } | null>;
}

const KNOWN_APP_DISPLAY_NAMES: Record<string, string> = {
  "com.1password.1password": "1Password",
  "com.agilebits.onepassword7": "1Password",
  "com.apple.keychainaccess": "Keychain Access",
  "com.bitwarden.desktop": "Bitwarden",
  "com.apple.safari": "Safari",
  "com.google.chrome": "Google Chrome",
  "org.mozilla.firefox": "Firefox",
  "com.apple.mail": "Mail",
  "com.apple.mobilesms": "Messages",
  "com.apple.terminal": "Terminal",
  "com.todesktop.230313mzl4w4u92": "Cursor",
};

export function formatBlockedAppName(
  appId: string,
  displayNames?: Record<string, string>,
): string {
  const fromMap = displayNames?.[appId]?.trim();
  if (fromMap) return fromMap;
  const trimmed = appId.trim();
  const known = KNOWN_APP_DISPLAY_NAMES[trimmed.toLowerCase()];
  if (known) return known;
  if (/^[a-z0-9-]+(\.[a-z0-9-]+){2,}$/i.test(trimmed)) {
    const tail = trimmed.split(".").pop() ?? trimmed;
    if (tail.toLowerCase() === "1password") return "1Password";
    return tail.charAt(0).toUpperCase() + tail.slice(1);
  }
  return trimmed;
}

export function formatBlockedSiteRow(site: BlockedSiteEntry | string): string {
  if (typeof site === "string") {
    return site.trim();
  }
  const name = typeof site?.name === "string" ? site.name.trim() : "";
  const browser =
    typeof site?.browser_identifier === "string"
      ? site.browser_identifier.trim()
      : "";
  const domain = typeof site?.domain === "string" ? site.domain.trim() : "";
  const details = [browser, domain].filter(Boolean).join(", ");
  if (name && details) return `${name} (${details})`;
  return name || details;
}

export const NeverDictateCard: React.FC<NeverDictateCardProps> = ({
  blockedApps,
  blockedSites = [],
  displayNames = {},
  onSaveBlockedApps,
  onSaveBlockedSites,
  onReadFrontApp,
}) => {
  const { t } = useTranslation();
  const [adding, setAdding] = useState(false);
  const [input, setInput] = useState("");
  const [siteDomain, setSiteDomain] = useState("");
  const [siteName, setSiteName] = useState("");

  const fillFront = async () => {
    const info = await onReadFrontApp?.();
    if (info) setInput(info.app_name || info.bundle_id || "");
  };

  const hasEntries = blockedApps.length > 0 || blockedSites.length > 0;
  const trimmedApp = input.trim();
  const trimmedDomain = siteDomain.trim().toLowerCase().replace(/\.$/, "");
  const trimmedSiteName = siteName.trim();
  const canAdd = trimmedDomain
    ? Boolean(trimmedApp && trimmedSiteName)
    : Boolean(trimmedApp);

  const handleAdd = () => {
    if (!canAdd) return;
    if (trimmedDomain && onSaveBlockedSites) {
      const nextSites: BlockedSiteEntry[] = [
        ...blockedSites,
        {
          name: trimmedSiteName,
          browser_identifier: trimmedApp,
          domain: trimmedDomain,
        },
      ];
      void onSaveBlockedSites(nextSites);
    } else if (trimmedApp) {
      void onSaveBlockedApps([...blockedApps, trimmedApp]);
    }
    setInput("");
    setSiteDomain("");
    setSiteName("");
    setAdding(false);
  };

  return (
    <section
      data-testid="never-dictate-card"
      aria-label={t("profiles.neverHeading")}
      className="mt-5 grid gap-2 rounded-xl border border-mid-gray/25 bg-background p-3.5"
    >
      <h3 className="text-[14px] font-bold text-text">
        {t("profiles.neverHeading")}
      </h3>
      {!hasEntries ? (
        <p className="text-[13px] text-mid-gray">{t("profiles.neverEmpty")}</p>
      ) : (
        <div className="divide-y divide-mid-gray/20">
          {blockedApps.map((app) => (
            <div
              key={app}
              className="flex items-center justify-between gap-4 py-2.5"
            >
              <div className="min-w-0">
                <p className="text-[14px] font-semibold text-text">
                  {formatBlockedAppName(app, displayNames)}
                </p>
                <p className="mt-0.5 text-[13px] text-mid-gray">
                  {t("profiles.neverRowDesc")}
                </p>
              </div>
              <button
                type="button"
                onClick={() =>
                  void onSaveBlockedApps(
                    blockedApps.filter((item) => item !== app),
                  )
                }
                className={OUTLINED_BTN_SM}
              >
                {t("profiles.remove")}
              </button>
            </div>
          ))}
          {blockedSites.map((site, idx) => (
            <div
              key={
                typeof site === "string"
                  ? `site:${site}:${idx}`
                  : `${site.browser_identifier ?? ""}:${site.domain ?? ""}:${idx}`
              }
              data-testid="never-dictate-site-row"
              className="flex items-center justify-between gap-4 py-2.5"
            >
              <div className="min-w-0">
                <p className="text-[14px] font-semibold text-text">
                  {formatBlockedSiteRow(site)}
                </p>
                <p className="mt-0.5 text-[13px] text-mid-gray">
                  {t("profiles.neverRowDesc")}
                </p>
              </div>
              <button
                type="button"
                onClick={() =>
                  onSaveBlockedSites &&
                  void onSaveBlockedSites(
                    blockedSites.filter(
                      (s) =>
                        !(
                          s.browser_identifier === site.browser_identifier &&
                          s.domain === site.domain
                        ),
                    ),
                  )
                }
                className={OUTLINED_BTN_SM}
              >
                {t("profiles.remove")}
              </button>
            </div>
          ))}
        </div>
      )}
      {adding ? (
        <div className="grid gap-2 pt-1">
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={t("profiles.appIdPlaceholder")}
              aria-label={t("profiles.appIdPlaceholder")}
              className={INPUT_CLS}
            />
            {onReadFrontApp ? (
              <button
                type="button"
                onClick={() => void fillFront()}
                className={OUTLINED_BTN_SM}
              >
                {t("profiles.useFrontApp")}
              </button>
            ) : null}
          </div>
          {onSaveBlockedSites ? (
            <div className="flex flex-wrap items-center gap-2">
              <input
                type="text"
                value={siteDomain}
                onChange={(e) => setSiteDomain(e.target.value)}
                placeholder={t(
                  "profiles.websitePlaceholder",
                  "Website (optional, e.g. bank.example)",
                )}
                aria-label={t("profiles.websiteLabel", "Website")}
                className={INPUT_CLS}
              />
              {siteDomain.trim() ? (
                <input
                  type="text"
                  value={siteName}
                  onChange={(e) => setSiteName(e.target.value)}
                  placeholder={t(
                    "profiles.websiteNamePlaceholder",
                    "Rule name (e.g. Banking)",
                  )}
                  aria-label={t("profiles.websiteNameLabel", "Rule name")}
                  className={INPUT_CLS}
                />
              ) : null}
            </div>
          ) : null}
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              disabled={!canAdd}
              onClick={handleAdd}
              className={OUTLINED_BTN_SM}
            >
              {t("profiles.addApp")}
            </button>
            <button
              type="button"
              onClick={() => {
                setInput("");
                setSiteDomain("");
                setSiteName("");
                setAdding(false);
              }}
              className={OUTLINED_BTN_SM}
            >
              {t("tray.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div className="pt-1">
          <button
            type="button"
            onClick={() => setAdding(true)}
            className={OUTLINED_BTN_SM}
          >
            {t("profiles.addApp")}
          </button>
        </div>
      )}
    </section>
  );
};
