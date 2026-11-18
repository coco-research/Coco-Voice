import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type AgentReadAuditEntry } from "../../bindings.meetings";
import { ToggleSwitch } from "../ui/ToggleSwitch";

const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
const MID_SEP = " · ";
export const MCP_STDIO_COMMAND = "coco-voice mcp";

export interface AgentAccessSectionProps {
  enabled: boolean;
  allowLiveMeeting: boolean;
  allowPastMeetings: boolean;
  recentReads: AgentReadAuditEntry[];
  onToggleEnabled: (enabled: boolean) => void;
  onToggleLiveMeeting: (allowed: boolean) => void;
  onTogglePastMeetings: (allowed: boolean) => void;
  onCopyCommand?: (cmd: string) => void;
}

export const AgentAccessSection: React.FC<AgentAccessSectionProps> = ({
  enabled,
  allowLiveMeeting,
  allowPastMeetings,
  recentReads,
  onToggleEnabled,
  onToggleLiveMeeting,
  onTogglePastMeetings,
  onCopyCommand,
}) => {
  const { t } = useTranslation();

  const handleCopy = () => {
    if (onCopyCommand) {
      onCopyCommand(MCP_STDIO_COMMAND);
      return;
    }
    void navigator?.clipboard?.writeText?.(MCP_STDIO_COMMAND);
  };

  return (
    <div
      data-testid="page-5-agent-access"
      className="max-w-3xl w-full mx-auto space-y-4"
    >
      <p className={EYEBROW}>{t("meetings.sidebar")}</p>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.agentAccess")}
        </h2>
        <span className="rounded-full border border-mid-gray/40 px-2.5 py-0.5 text-[11.5px] font-semibold text-mid-gray">
          {t("meetings.agent.offByDefault")}
        </span>
      </div>
      <p className="text-[13px] leading-[19px] text-mid-gray">
        {t("meetings.agent.subtitle")}
      </p>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-2">
        <ToggleSwitch
          checked={enabled}
          onChange={onToggleEnabled}
          label={t("meetings.agent.masterLabel")}
          description={t("meetings.agent.offByDefault")}
          descriptionMode="inline"
          grouped={true}
        />
      </section>

      <section
        data-testid="agent-scope-card"
        aria-disabled={!enabled}
        className={`rounded-[14px] border border-mid-gray/25 bg-background p-3 space-y-1 transition-opacity ${
          enabled ? "" : "opacity-50 pointer-events-none"
        }`}
      >
        <h3 className="px-2 pt-1 text-[14px] font-semibold text-text">
          {t("meetings.agent.canReadHeading")}
        </h3>
        <ToggleSwitch
          checked={enabled && allowLiveMeeting}
          disabled={!enabled}
          onChange={onToggleLiveMeeting}
          label={t("meetings.agent.liveMeetingLabel")}
          description={t("meetings.retention.onThisMac")}
          descriptionMode="inline"
          grouped={true}
        />
        <ToggleSwitch
          checked={enabled && allowPastMeetings}
          disabled={!enabled}
          onChange={onTogglePastMeetings}
          label={t("meetings.agent.pastMeetingsLabel")}
          description={t("meetings.retention.onThisMac")}
          descriptionMode="inline"
          grouped={true}
        />
        <div className="flex items-center justify-between px-3 py-2.5 border-t border-mid-gray/15">
          <span className="text-[13.5px] font-medium text-text">
            {t("meetings.agent.audioLabel")}
          </span>
          <span className="font-mono text-[12px] font-semibold text-mid-gray">
            {t("meetings.agent.audioNever")}
          </span>
        </div>
      </section>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5">
        <h3 className="text-[15px] font-semibold text-text">
          {t("meetings.agent.connectHeading")}
        </h3>
        <div className="flex items-center justify-between gap-2 rounded-[10px] border border-mid-gray/30 bg-mid-gray/[0.06] px-3 py-2">
          <code className="font-mono text-[13px] font-semibold text-text">
            {MCP_STDIO_COMMAND}
          </code>
          <button
            type="button"
            onClick={handleCopy}
            className="rounded-[8px] border border-mid-gray/45 bg-background px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.agent.copyCommand")}
          </button>
        </div>
      </section>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2">
        <h3 className="text-[15px] font-semibold text-text">
          {t("meetings.agent.recentReadsHeading")}
        </h3>
        <ul className="divide-y divide-mid-gray/20 text-[13px]">
          {recentReads.map((entry) => {
            const meta = `${entry.agent_name}${MID_SEP}${entry.when_label}`;
            return (
              <li
                key={entry.id}
                className="flex items-center justify-between py-2"
              >
                <span className="font-mono text-[12.5px] font-semibold text-text">
                  {entry.tool}
                </span>
                <span className="text-[12px] text-mid-gray">{meta}</span>
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
};

const SAMPLE_READS: AgentReadAuditEntry[] = [
  {
    id: "r-1",
    tool: "list_meetings",
    agent_name: "Claude Code",
    when_label: "14:22",
  },
];

export const AgentAccessView: React.FC = () => {
  // Always off by default until the user explicitly turns it on.
  const [enabled, setEnabled] = useState(false);
  const [live, setLive] = useState(true);
  const [past, setPast] = useState(true);
  const [reads, setReads] = useState<AgentReadAuditEntry[]>(SAMPLE_READS);

  useEffect(() => {
    let cancelled = false;
    void commands.agentAccessState().then((res) => {
      if (!cancelled && res.status === "ok") {
        setEnabled(res.data.enabled);
        setLive(res.data.allow_live_meeting);
        setPast(res.data.allow_past_meetings);
        setReads(res.data.recent_reads);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <AgentAccessSection
      enabled={enabled}
      allowLiveMeeting={live}
      allowPastMeetings={past}
      recentReads={reads}
      onToggleEnabled={(next) => {
        setEnabled(next);
        void commands.setAgentAccessEnabled(next);
      }}
      onToggleLiveMeeting={(next) => {
        setLive(next);
        void commands.setAgentAccessScope(next, past);
      }}
      onTogglePastMeetings={(next) => {
        setPast(next);
        void commands.setAgentAccessScope(live, next);
      }}
    />
  );
};
