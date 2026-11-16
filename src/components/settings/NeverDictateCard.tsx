import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { OUTLINED_BTN_SM } from "../meetings/MeetingsList";

const INPUT_CLS =
  "flex-1 min-w-[200px] rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text";

export interface NeverDictateCardProps {
  blockedApps: string[];
  onSaveBlockedApps: (next: string[]) => Promise<void>;
  onReadFrontApp?: () => Promise<{
    app_name: string;
    bundle_id: string | null;
  } | null>;
}

export const NeverDictateCard: React.FC<NeverDictateCardProps> = ({
  blockedApps,
  onSaveBlockedApps,
  onReadFrontApp,
}) => {
  const { t } = useTranslation();
  const [adding, setAdding] = useState(false);
  const [input, setInput] = useState("");

  const fillFront = async () => {
    const info = await onReadFrontApp?.();
    if (info) setInput(info.app_name || info.bundle_id || "");
  };

  return (
    <section
      aria-label={t("profiles.neverHeading")}
      className="mt-5 grid gap-2 rounded-xl border border-mid-gray/25 bg-background p-3.5"
    >
      <h3 className="text-[14px] font-bold text-text">
        {t("profiles.neverHeading")}
      </h3>
      {blockedApps.length === 0 ? (
        <p className="text-[13px] text-mid-gray">{t("profiles.neverEmpty")}</p>
      ) : (
        <div className="divide-y divide-mid-gray/20">
          {blockedApps.map((app) => (
            <div
              key={app}
              className="flex items-center justify-between gap-4 py-2.5"
            >
              <div className="min-w-0">
                <p className="text-[14px] font-semibold text-text">{app}</p>
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
        </div>
      )}
      {adding ? (
        <div className="flex flex-wrap items-center gap-2 pt-1">
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
          <button
            type="button"
            disabled={!input.trim()}
            onClick={() => {
              const v = input.trim();
              if (!v) return;
              void onSaveBlockedApps([...blockedApps, v]);
              setInput("");
              setAdding(false);
            }}
            className={OUTLINED_BTN_SM}
          >
            {t("profiles.addApp")}
          </button>
          <button
            type="button"
            onClick={() => {
              setInput("");
              setAdding(false);
            }}
            className={OUTLINED_BTN_SM}
          >
            {t("tray.cancel")}
          </button>
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
