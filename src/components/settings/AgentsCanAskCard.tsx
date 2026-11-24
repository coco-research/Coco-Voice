import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";

export const MCP_ASK_USER_STDIO_COMMAND = "coco-voice --mcp-ask-user";

export interface AgentsCanAskSectionProps {
  enabled: boolean;
  onToggleEnabled: (next: boolean) => void;
  onCopyCommand?: (cmd: string) => void;
}

export const AgentsCanAskSection: React.FC<AgentsCanAskSectionProps> = ({
  enabled,
  onToggleEnabled,
  onCopyCommand,
}) => {
  const { t } = useTranslation();

  const handleCopy = () => {
    if (onCopyCommand) {
      onCopyCommand(MCP_ASK_USER_STDIO_COMMAND);
      return;
    }
    void navigator?.clipboard?.writeText?.(MCP_ASK_USER_STDIO_COMMAND);
  };

  return (
    <div data-testid="agents-can-ask-card">
      <SettingsGroup title={t("settings.agentsCanAsk.title")}>
        <ToggleSwitch
          checked={enabled}
          onChange={onToggleEnabled}
          label={t("settings.agentsCanAsk.label")}
          description={t("settings.agentsCanAsk.description")}
          descriptionMode="inline"
          grouped={true}
        />
        <div
          data-testid={
            enabled ? "agents-can-ask-state-on" : "agents-can-ask-state-off"
          }
          className="flex flex-wrap items-center justify-between gap-2 px-4 py-3 border-t border-mid-gray/15"
        >
          <span className="text-xs text-mid-gray">
            {enabled
              ? t("settings.agentsCanAsk.connectLabel")
              : t("settings.agentsCanAsk.offByDefault")}
          </span>
          <div className="flex items-center gap-2">
            <code className="rounded-md border border-mid-gray/30 bg-mid-gray/[0.06] px-2 py-0.5 font-mono text-xs font-semibold text-text">
              {MCP_ASK_USER_STDIO_COMMAND}
            </code>
            <button
              type="button"
              onClick={handleCopy}
              className="rounded-lg border border-mid-gray/45 bg-background px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("settings.agentsCanAsk.copyCommand")}
            </button>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
};

export const AgentsCanAskCard: React.FC = () => {
  const { settings, updateSetting } = useSettings();
  const enabled = Boolean(
    settings?.agents_can_ask_enabled ?? settings?.agent_ask_enabled,
  );

  return (
    <AgentsCanAskSection
      enabled={enabled}
      onToggleEnabled={(next) => {
        void updateSetting("agents_can_ask_enabled", next);
      }}
    />
  );
};
