import React from "react";
import { useTranslation } from "react-i18next";
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
  <div className="flex items-baseline justify-between border-b border-text/75 pb-1.5">
    <h3 className="text-[15px] leading-[20px] font-bold text-text">
      {p.title}
    </h3>
    <span className="font-mono text-[12px] text-mid-gray">{p.count}</span>
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
  <div className="flex flex-wrap items-start justify-between gap-3 py-3">
    <div className="min-w-0 flex-1">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <span className="text-[14px] font-semibold text-text">{p.name}</span>
        <code
          data-online-host={p.host}
          className="font-mono text-[12px] text-mid-gray"
        >
          {p.host}
        </code>
      </div>
      <p className="mt-0.5 text-[13px] leading-[19px] text-mid-gray">
        {p.when}
      </p>
      <p className="mt-0.5 text-[13px] leading-[19px] text-text/80">
        <strong className="font-semibold text-text">{p.label}</strong>{" "}
        {p.leaves}
      </p>
    </div>
    {p.badge ? (
      <span
        className={`inline-flex shrink-0 items-center gap-1.5 pt-0.5 font-mono text-[11.5px] font-semibold ${p.tone}`}
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
      className="space-y-4 rounded-lg border border-mid-gray/20 bg-background p-5"
    >
      <div>
        <p className="text-[11px] font-semibold uppercase tracking-[0.08em] text-logo-primary">
          {t("tray.settings").replace(/\.+$/, "")}
        </p>
        <h2
          id="what-goes-online-heading"
          className="mt-1 text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text"
        >
          {t("settings.online.title")}
        </h2>
        <p className="mt-1.5 text-[13px] leading-[18px] text-mid-gray">
          {t("settings.online.intro")}
        </p>
      </div>

      <div data-online-group="open">
        <GroupHead
          title={t("settings.online.openHeading")}
          count={ONLINE_OPEN_HOSTS.length}
        />
        <div className="border-b border-mid-gray/25">
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
        <div className="divide-y divide-mid-gray/20 border-b border-mid-gray/25">
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
        <div className="divide-y divide-mid-gray/25 border-b border-mid-gray/25">
          <div data-online-cloud-group="true" className="py-3">
            <p className="text-[13px] leading-[19px] text-mid-gray">
              {t("settings.online.cloudWhen")}
            </p>
            <p className="mt-0.5 text-[13px] leading-[19px] text-text/80">
              <strong className="font-semibold text-text">{label}</strong>{" "}
              {t("settings.online.cloudLeaves")}
            </p>
            <div className="mt-2.5 divide-y divide-mid-gray/15 border-t border-mid-gray/20">
              {ONLINE_CLOUD_PROVIDERS.map((p) => (
                <div
                  key={p.host}
                  data-online-cloud-row={p.host}
                  className="flex flex-wrap items-baseline gap-x-2 py-2"
                >
                  <span className="text-[14px] font-semibold text-text">
                    {p.name}
                  </span>
                  <code
                    data-online-host={p.host}
                    className="font-mono text-[12px] text-mid-gray"
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

      <p className="pt-0.5 text-[13.5px] leading-[20px] text-text">
        {t("settings.online.onDeviceNote")}
      </p>
    </section>
  );
};

export const WhatGoesOnline: React.FC = () => {
  const { getSetting } = useSettings();
  return (
    <WhatGoesOnlineSection
      updateChecksEnabled={getSetting("update_checks_enabled") ?? true}
    />
  );
};
