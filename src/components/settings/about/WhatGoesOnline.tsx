import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { useSettings } from "../../../hooks/useSettings";

export const ONLINE_EXCLUDED_WHEN = [
  "not contacted by the app",
  "only when a Windows build signs the installer",
] as const;

export const ONLINE_WHEN_BY_GROUP = {
  open: "always",
  click: "only when they click a button",
  cleanup: "only after the user turns something on",
} as const;

export const ONLINE_OPEN_HOSTS = ["github.com"] as const;

export const ONLINE_CLICK_ROWS = [
  { host: "blob.handy.computer", k: "modelFiles" },
  { host: "huggingface.co", k: "huggingFace", brand: "Hugging Face" },
] as const;

export const ONLINE_CLOUD_PROVIDERS = [
  { name: "Anthropic", host: "api.anthropic.com" },
  { name: "Cerebras", host: "api.cerebras.ai" },
  { name: "Groq", host: "api.groq.com" },
  { name: "OpenAI", host: "api.openai.com" },
  { name: "Z.AI", host: "api.z.ai" },
  { name: "AWS Bedrock", host: "bedrock-mantle.us-east-1.api.aws" },
  { name: "OpenRouter", host: "openrouter.ai" },
] as const;

export const ONLINE_CUSTOM_HOST = "localhost";

const GroupHead: React.FC<{ title: string; count: number }> = (p) => (
  <div className="flex items-baseline justify-between border-b border-mid-gray/25 pb-1">
    <h3 className="text-[13px] leading-[18px] font-bold text-text">
      {p.title}
    </h3>
    <span className="font-mono text-[11px] text-mid-gray">{p.count}</span>
  </div>
);

const HostRow: React.FC<{
  name: string;
  host: string;
  when: string;
  label: string;
  leaves: string;
  badge?: string;
  tone?: string;
}> = (p) => (
  <div className="flex flex-wrap items-start justify-between gap-2 py-1.5">
    <div className="min-w-0 flex-1">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <span className="text-[13px] font-semibold text-text">{p.name}</span>
        <code
          data-online-host={p.host}
          className="font-mono text-[11.5px] text-mid-gray"
        >
          {p.host}
        </code>
      </div>
      <p className="text-[12px] leading-[16px] text-mid-gray">
        {p.when} ·{" "}
        <strong className="font-semibold text-text">{p.label}</strong>{" "}
        {p.leaves}
      </p>
    </div>
    {p.badge ? (
      <span
        className={`inline-flex shrink-0 items-center gap-1 font-mono text-[11px] font-semibold ${p.tone}`}
      >
        <span
          aria-hidden="true"
          className="inline-block h-1.5 w-1.5 rounded-full bg-current"
        />
        {p.badge}
      </span>
    ) : null}
  </div>
);

export const WhatGoesOnlineSection: React.FC<{
  updateChecksEnabled: boolean;
}> = ({ updateChecksEnabled }) => {
  const { t } = useTranslation();
  const label = t("settings.online.leavesLabel");
  const badgeKey = updateChecksEnabled ? "updateChecksOn" : "updateChecksOff";
  const badgeTone = updateChecksEnabled ? "text-logo-primary" : "text-mid-gray";

  return (
    <section
      aria-labelledby="what-goes-online-heading"
      data-testid="what-goes-online"
      className="space-y-2.5 rounded-xl border border-mid-gray/25 bg-surface px-4 py-3"
    >
      <div>
        <div className="flex items-baseline justify-between gap-2">
          <h2
            id="what-goes-online-heading"
            className="text-[14px] leading-[20px] font-bold text-text"
          >
            {t("settings.online.title")}
          </h2>
          <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-logo-primary">
            {t("settings.online.eyebrow")}
          </span>
        </div>
        <p className="mt-0.5 text-[12.5px] leading-[17px] text-mid-gray">
          {t("settings.online.intro")}
        </p>
      </div>

      <div data-online-group="open">
        <GroupHead
          title={t("settings.online.openHeading")}
          count={ONLINE_OPEN_HOSTS.length}
        />
        <div>
          <HostRow
            name={t("settings.online.githubName")}
            host={ONLINE_OPEN_HOSTS[0]}
            when={t("settings.online.githubWhen")}
            label={label}
            leaves={t("settings.online.githubLeaves")}
            badge={t(`settings.online.${badgeKey}`)}
            tone={badgeTone}
          />
        </div>
      </div>

      <div data-online-group="click">
        <GroupHead
          title={t("settings.online.clickHeading")}
          count={ONLINE_CLICK_ROWS.length}
        />
        <div className="divide-y divide-mid-gray/15">
          {ONLINE_CLICK_ROWS.map((r) => (
            <HostRow
              key={r.host}
              name={"brand" in r ? r.brand : t(`settings.online.${r.k}Name`)}
              host={r.host}
              when={t(`settings.online.${r.k}When`)}
              label={label}
              leaves={t(`settings.online.${r.k}Leaves`)}
            />
          ))}
        </div>
      </div>

      <div data-online-group="cleanup">
        <GroupHead
          title={t("settings.online.cleanupHeading")}
          count={ONLINE_CLOUD_PROVIDERS.length + 1}
        />
        <div className="divide-y divide-mid-gray/15">
          <div data-online-cloud-group="true" className="py-1.5">
            <p className="text-[12px] leading-[16px] text-mid-gray">
              {t("settings.online.cloudWhen")} ·{" "}
              <strong className="font-semibold text-text">{label}</strong>{" "}
              {t("settings.online.cloudLeaves")}
            </p>
            <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5">
              {ONLINE_CLOUD_PROVIDERS.map((p) => (
                <div
                  key={p.host}
                  data-online-cloud-row={p.host}
                  className="inline-flex items-baseline gap-x-1"
                >
                  <span className="text-[11.5px] font-semibold text-text">
                    {p.name}
                  </span>
                  <code
                    data-online-host={p.host}
                    className="font-mono text-[11px] text-mid-gray"
                  >
                    {p.host}
                  </code>
                </div>
              ))}
            </div>
          </div>
          <HostRow
            name={t("settings.online.customName")}
            host={ONLINE_CUSTOM_HOST}
            when={t("settings.online.customWhen")}
            label={label}
            leaves={t("settings.online.customLeaves")}
            badge={t("settings.online.staysOnMac")}
            tone="text-emerald-700 dark:text-emerald-400"
          />
        </div>
      </div>

      <p className="border-t border-mid-gray/20 pt-1.5 text-[12px] leading-[17px] text-mid-gray">
        {t("settings.online.onDeviceNote")}
      </p>
    </section>
  );
};

export const WhatGoesOnline: React.FC = () => {
  const { getSetting } = useSettings();
  // ponytail: macOS-only copy; add Windows/Linux wording when those builds need it
  const isMacOS = type() === "macos";
  if (!isMacOS) {
    return null;
  }
  return (
    <WhatGoesOnlineSection
      updateChecksEnabled={getSetting("update_checks_enabled") ?? true}
    />
  );
};

export const WhatGoesOnlineSummaryCard: React.FC<{
  updateChecksEnabled?: boolean;
  onOpenSettings?: () => void;
}> = ({ updateChecksEnabled: updateChecksProp, onOpenSettings }) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const updateChecksEnabled =
    updateChecksProp !== undefined
      ? updateChecksProp
      : (getSetting("update_checks_enabled") ?? true);

  return (
    <section
      data-testid="what-goes-online-summary"
      aria-label={t("settings.online.title")}
      className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3.5 space-y-2"
    >
      <h2 className="text-[14px] font-bold text-text">
        {t("settings.online.title")}
      </h2>
      <p className="text-[13px] leading-[18px] text-mid-gray">
        {t(
          "v022.about.onlineSummaryIntro",
          "Your speech, meetings and notes stay on this Mac. These are the only places the app contacts:",
        )}
      </p>
      <div className="divide-y divide-mid-gray/15 text-[13px]">
        <div className="py-1.5" data-testid="online-summary-updates-row">
          {updateChecksEnabled ? (
            <>
              <div className="font-medium text-text">
                {t("v022.about.everyLaunchTitle", "Every launch")}
              </div>
              <div className="text-xs text-mid-gray">
                <span>
                  {t(
                    "v022.about.githubUpdateCheckPrefix",
                    "github.com (update check, ",
                  )}
                </span>
                {onOpenSettings ? (
                  <button
                    type="button"
                    onClick={onOpenSettings}
                    className="underline text-logo-primary cursor-pointer hover:opacity-80"
                  >
                    {t(
                      "v022.about.turnItOffInSettings",
                      "turn it off in Settings",
                    )}
                  </button>
                ) : (
                  <span>
                    {t(
                      "v022.about.turnItOffInSettings",
                      "turn it off in Settings",
                    )}
                  </span>
                )}
                <span>{")"}</span>
              </div>
            </>
          ) : (
            <>
              <div className="font-medium text-text">
                {t("v022.about.onlyWhenClickTitle", "Only when you click")}
              </div>
              <div className="text-xs text-mid-gray">
                {t(
                  "v022.about.updatesOffSummaryDesc",
                  "github.com (update check is off in Settings)",
                )}
              </div>
            </>
          )}
        </div>
        <div className="py-1.5">
          <div className="font-medium text-text">
            {t("v022.about.onlyWhenClickTitle", "Only when you click")}
          </div>
          <div className="text-xs text-mid-gray">
            {t("v022.about.onlyWhenClickDesc", {
              modelHost: ONLINE_CLICK_ROWS[0].host,
              hfHost: ONLINE_CLICK_ROWS[1].host,
              defaultValue: "{{modelHost}}, {{hfHost}} (model downloads)",
            })}
          </div>
        </div>
        <div className="py-1.5" data-testid="online-summary-cloud-row">
          <div className="font-medium text-text">
            {t(
              "v022.about.onlyWhenCloudTitle",
              "Only when you pick a cloud clean-up provider",
            )}
          </div>
          <div className="text-xs text-mid-gray">
            {t("v022.about.onlyWhenCloudDescCount", {
              count: ONLINE_CLOUD_PROVIDERS.length,
              defaultValue: `${ONLINE_CLOUD_PROVIDERS.length} providers, or your own server on localhost`,
            })}
          </div>
        </div>
      </div>
      <p className="text-xs leading-[18px] text-mid-gray">
        {t(
          "v022.about.onDeviceContactsNone",
          "On-device clean-up (Apple Intelligence, or the local model) contacts none of these.",
        )}
      </p>
    </section>
  );
};
