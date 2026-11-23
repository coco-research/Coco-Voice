import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AgentReadAuditEntry,
  type Result,
} from "../../bindings.meetings";
import { ToggleSwitch } from "../ui/ToggleSwitch";

const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
export const MCP_STDIO_COMMAND = "coco-voice mcp";
const MAX_VISIBLE_READS = 5;
const COPY_FEEDBACK_MS = 2000;
const POLL_INTERVAL_MS = 5000;

export type AgentReadKind = "notes" | "transcript" | "live" | "list";

export interface AgentReadAuditRow {
  id: string;
  client: string;
  kind: AgentReadKind;
  target_title: string;
  ts: number;
}

export interface LegacyAgentReadAuditEntry {
  id: string;
  tool?: string;
  agent_name?: string;
  when_label?: string;
}

export type AgentReadInput =
  AgentReadAuditRow | AgentReadAuditEntry | LegacyAgentReadAuditEntry;

export interface AgentAccessStateSnapshot {
  enabled: boolean;
  allow_live_meeting: boolean;
  allow_past_meetings: boolean;
  recent_reads: AgentReadInput[];
}

export interface AgentAccessClient {
  agentAccessState: () => Promise<Result<AgentAccessStateSnapshot>>;
  setAgentAccessEnabled: (
    enabled: boolean,
  ) => Promise<Result<AgentAccessStateSnapshot>>;
  setAgentAccessScope: (
    allowLiveMeeting: boolean,
    allowPastMeetings: boolean,
  ) => Promise<Result<AgentAccessStateSnapshot>>;
  mcpCommandLine: () => Promise<Result<string>>;
}

const KNOWN_CLIENT_NAMES: Record<string, string> = {
  "claude-code": "Claude Code",
  cursor: "Cursor",
  codex: "Codex",
};

export function formatAgentClientName(client: string): string {
  const trimmed = client.trim();
  const mapped = KNOWN_CLIENT_NAMES[trimmed.toLowerCase()];
  return mapped ?? (trimmed || client);
}

function isStructuredReadRow(raw: AgentReadInput): raw is AgentReadAuditRow {
  return (
    typeof (raw as AgentReadAuditRow).kind === "string" &&
    typeof (raw as AgentReadAuditRow).ts === "number" &&
    typeof (raw as AgentReadAuditRow).client === "string"
  );
}

export function normalizeAgentReadRow(raw: AgentReadInput): AgentReadAuditRow {
  if (isStructuredReadRow(raw)) {
    const k = raw.kind;
    const kind: AgentReadKind =
      k === "notes" || k === "transcript" || k === "live" || k === "list"
        ? k
        : "notes";
    return {
      id: raw.id,
      client: raw.client,
      kind,
      target_title: raw.target_title ?? "",
      ts: raw.ts,
    };
  }
  const legacyTool = raw.tool ?? "";
  const kind: AgentReadKind =
    legacyTool === "live_meeting_transcript"
      ? "live"
      : legacyTool === "list_meetings"
        ? "list"
        : legacyTool === "get_meeting_transcript"
          ? "transcript"
          : "notes";
  const parsedTs = Number.parseInt(raw.when_label ?? "", 10);
  return {
    id: raw.id,
    client: raw.agent_name ?? "",
    kind,
    target_title: "",
    ts: Number.isFinite(parsedTs) ? parsedTs : 0,
  };
}

function formatLocalHHMM(date: Date): string {
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

function sameLocalDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function formatAgentReadWhen(
  tsSeconds: number,
  t: TFunction,
  nowMs?: number,
  locale?: string,
): string {
  const date = new Date(tsSeconds * 1000);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  const now = new Date(nowMs ?? Date.now());
  const time = formatLocalHHMM(date);
  if (sameLocalDay(date, now)) {
    return t("meetings.agent.whenToday", "Today {{time}}", {
      time,
      interpolation: { escapeValue: false },
    });
  }
  const yesterday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() - 1,
  );
  if (sameLocalDay(date, yesterday)) {
    return t("meetings.agent.whenYesterday", "Yesterday {{time}}", {
      time,
      interpolation: { escapeValue: false },
    });
  }
  const resolvedLocale = locale || undefined;
  const parts = new Intl.DateTimeFormat(resolvedLocale, {
    weekday: "short",
    day: "numeric",
    month: "short",
  }).formatToParts(date);
  const weekday = parts.find((p) => p.type === "weekday")?.value ?? "";
  const day = parts.find((p) => p.type === "day")?.value ?? "";
  const month = parts.find((p) => p.type === "month")?.value ?? "";
  return t(
    "meetings.agent.whenDate",
    "{{weekday}} {{day}} {{month}} {{time}}",
    {
      weekday,
      day,
      month,
      time,
      interpolation: { escapeValue: false },
    },
  );
}

export function formatAgentReadLine(
  entry: AgentReadAuditRow,
  t: TFunction,
  nowMs?: number,
  locale?: string,
): string {
  const agent = formatAgentClientName(entry.client);
  const when = formatAgentReadWhen(entry.ts, t, nowMs, locale);
  const meeting = entry.target_title;
  const opts = {
    agent,
    meeting,
    when,
    interpolation: { escapeValue: false },
  };
  switch (entry.kind) {
    case "notes":
      return t(
        "meetings.agent.readNotes",
        '{{agent}} read "{{meeting}}" notes · {{when}}',
        opts,
      );
    case "transcript":
      return t(
        "meetings.agent.readTranscript",
        '{{agent}} read "{{meeting}}" transcript · {{when}}',
        opts,
      );
    case "live":
      if (meeting) {
        return t(
          "meetings.agent.readLive",
          '{{agent}} read the live transcript of "{{meeting}}" · {{when}}',
          opts,
        );
      }
      return t(
        "meetings.agent.readLiveNoMeeting",
        "{{agent}} checked for a live meeting · {{when}}",
        opts,
      );
    case "list":
      return t(
        "meetings.agent.readList",
        "{{agent}} listed your meetings · {{when}}",
        opts,
      );
  }
}

export async function resolveAndCopyMcpCommand(
  client: Pick<AgentAccessClient, "mcpCommandLine">,
  writeClipboard: (text: string) => void | Promise<void>,
): Promise<{ command: string; usedFallback: boolean }> {
  let resolved = MCP_STDIO_COMMAND;
  try {
    const res = await client.mcpCommandLine();
    if (res.status === "ok" && res.data.trim()) {
      resolved = res.data.trim();
    }
  } catch {
    resolved = MCP_STDIO_COMMAND;
  }
  await writeClipboard(resolved);
  return {
    command: resolved,
    usedFallback: resolved === MCP_STDIO_COMMAND,
  };
}

export async function applyAgentAccessMutation(
  previous: AgentAccessStateSnapshot,
  mutate: () => Promise<Result<AgentAccessStateSnapshot>>,
): Promise<{ snapshot: AgentAccessStateSnapshot; failed: boolean }> {
  try {
    const res = await mutate();
    if (res.status === "ok") {
      return {
        snapshot: {
          enabled: res.data.enabled,
          allow_live_meeting: res.data.allow_live_meeting,
          allow_past_meetings: res.data.allow_past_meetings,
          recent_reads: res.data.recent_reads
            .slice(0, MAX_VISIBLE_READS)
            .map(normalizeAgentReadRow),
        },
        failed: false,
      };
    }
    return { snapshot: previous, failed: true };
  } catch {
    return { snapshot: previous, failed: true };
  }
}

export const defaultAgentAccessClient: AgentAccessClient = {
  agentAccessState: () =>
    commands.agentAccessState() as Promise<Result<AgentAccessStateSnapshot>>,
  setAgentAccessEnabled: (enabled: boolean) =>
    commands.setAgentAccessEnabled(enabled) as Promise<
      Result<AgentAccessStateSnapshot>
    >,
  setAgentAccessScope: (
    allowLiveMeeting: boolean,
    allowPastMeetings: boolean,
  ) =>
    commands.setAgentAccessScope(
      allowLiveMeeting,
      allowPastMeetings,
    ) as Promise<Result<AgentAccessStateSnapshot>>,
  mcpCommandLine: async () => {
    try {
      const cmd = await invoke<string>("mcp_command_line");
      return { status: "ok", data: cmd };
    } catch (err) {
      return { status: "error", error: String(err) };
    }
  },
};

export type SaveErrorSlot = "master" | "live" | "past" | null;

export interface AgentAccessSectionProps {
  enabled: boolean;
  allowLiveMeeting: boolean;
  allowPastMeetings: boolean;
  recentReads: AgentReadInput[];
  onToggleEnabled: (enabled: boolean) => void;
  onToggleLiveMeeting: (allowed: boolean) => void;
  onTogglePastMeetings: (allowed: boolean) => void;
  onCopyCommand?: () => void | Promise<void>;
  copied?: boolean;
  copyFallbackHint?: boolean;
  saveErrorSlot?: SaveErrorSlot;
  saveErrorText?: string | null;
  nowMs?: number;
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
  copied = false,
  copyFallbackHint = false,
  saveErrorSlot = null,
  saveErrorText = null,
  nowMs,
}) => {
  const { t, i18n } = useTranslation();
  const visibleReads = recentReads
    .slice(0, MAX_VISIBLE_READS)
    .map(normalizeAgentReadRow);

  const handleCopy = () => {
    if (onCopyCommand) {
      void onCopyCommand();
      return;
    }
    void navigator?.clipboard?.writeText?.(MCP_STDIO_COMMAND);
  };

  return (
    <div
      data-testid="page-5-agent-access"
      className="max-w-3xl w-full mx-auto space-y-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className={EYEBROW}>{t("meetings.sidebar")}</p>
          <h2 className="mt-1 text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
            {t("sidebar.agentAccess")}
          </h2>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1 pt-1 whitespace-nowrap">
          <ToggleSwitch
            checked={enabled}
            onChange={onToggleEnabled}
            label={t("meetings.agent.offByDefault")}
            description={t("meetings.agent.masterLabel")}
            descriptionMode="tooltip"
            grouped={true}
          />
          {saveErrorSlot === "master" && saveErrorText ? (
            <p
              role="alert"
              data-testid="agent-save-error"
              className="text-[12px] text-red-500"
            >
              {saveErrorText}
            </p>
          ) : null}
        </div>
      </div>

      <p className="text-[13px] leading-[19px] text-mid-gray">
        {t("meetings.agent.subtitle")}
      </p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 items-stretch">
        <section
          data-testid="agent-scope-card"
          aria-disabled={!enabled}
          className={`rounded-[14px] border border-mid-gray/25 bg-background p-3.5 space-y-1.5 transition-opacity ${
            enabled ? "" : "opacity-50 pointer-events-none"
          }`}
        >
          <h3 className="px-2 pt-0.5 text-[14px] font-semibold text-text">
            {t("meetings.agent.canReadHeading")}
          </h3>
          <div data-testid="agent-switch-live" aria-disabled={!enabled}>
            <ToggleSwitch
              checked={enabled && allowLiveMeeting}
              disabled={!enabled}
              onChange={onToggleLiveMeeting}
              label={t("meetings.agent.liveMeetingLabel")}
              description={t(
                "meetings.agent.liveMeetingDescription",
                "Live transcript, updated every few seconds",
              )}
              descriptionMode="inline"
              grouped={true}
            />
            {saveErrorSlot === "live" && saveErrorText ? (
              <p
                role="alert"
                data-testid="agent-save-error"
                className="px-3 text-[12px] text-red-500"
              >
                {saveErrorText}
              </p>
            ) : null}
          </div>
          <div data-testid="agent-switch-past" aria-disabled={!enabled}>
            <ToggleSwitch
              checked={enabled && allowPastMeetings}
              disabled={!enabled}
              onChange={onTogglePastMeetings}
              label={t("meetings.agent.pastMeetingsLabel")}
              description={t(
                "meetings.agent.pastMeetingsDescription",
                "Notes and transcripts",
              )}
              descriptionMode="inline"
              grouped={true}
            />
            {saveErrorSlot === "past" && saveErrorText ? (
              <p
                role="alert"
                data-testid="agent-save-error"
                className="px-3 text-[12px] text-red-500"
              >
                {saveErrorText}
              </p>
            ) : null}
          </div>
          <div className="flex items-center justify-between px-3 py-2 border-t border-mid-gray/15">
            <span className="text-[13px] font-medium text-text">
              {t("meetings.agent.audioLabel")}
            </span>
            <span className="text-[12px] font-semibold text-mid-gray">
              {t("meetings.agent.audioNever")}
            </span>
          </div>
          <p className="px-2 pt-1 text-[11.5px] leading-[16px] text-mid-gray">
            {t(
              "meetings.agent.disabledSwitchesHint",
              "When Agent access is off, these two switches show greyed out and can't be changed.",
            )}
          </p>
        </section>

        <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 flex flex-col justify-between space-y-3">
          <div className="space-y-2">
            <h3 className="text-[14px] font-semibold text-text">
              {t("meetings.agent.connectHeading")}
            </h3>
            <p className="text-[12.5px] text-mid-gray">
              {t(
                "meetings.agent.connectSubtitle",
                "Add this to your agent's MCP settings:",
              )}
            </p>
            <div className="rounded-[10px] border border-mid-gray/30 bg-mid-gray/[0.06] px-3 py-2.5">
              <code className="font-mono text-[13px] font-semibold text-text">
                {MCP_STDIO_COMMAND}
              </code>
            </div>
          </div>
          <div className="space-y-1.5">
            <button
              type="button"
              data-testid="agent-copy-command-btn"
              onClick={handleCopy}
              className="rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {copied
                ? t("meetings.agent.copiedCommand", "Copied")
                : t("meetings.agent.copyCommand", "Copy")}
            </button>
            {copyFallbackHint ? (
              <p
                data-testid="agent-copy-fallback-hint"
                className="text-[11.5px] leading-[16px] text-mid-gray"
              >
                {t(
                  "meetings.agent.copyFallbackHint",
                  "Use the full path to Coco Voice if your agent can't find it.",
                )}
              </p>
            ) : null}
          </div>
        </section>
      </div>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2">
        <h3 className="text-[14px] font-semibold text-text">
          {t("meetings.agent.recentReadsHeading")}
        </h3>
        {visibleReads.length === 0 ? (
          <p
            data-testid="agent-recent-reads-empty"
            className="text-[12.5px] text-mid-gray py-1"
          >
            {t(
              "meetings.agent.recentReadsEmpty",
              "No agent has read anything yet.",
            )}
          </p>
        ) : (
          <ul
            data-testid="agent-recent-reads-list"
            className="divide-y divide-mid-gray/20 text-[13px]"
          >
            {visibleReads.map((entry) => (
              <li
                key={entry.id}
                className="py-2 text-[13px] leading-[18px] text-text"
              >
                {formatAgentReadLine(entry, t, nowMs, i18n.language)}
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
};

export interface AgentAccessViewProps {
  client?: AgentAccessClient;
  writeClipboard?: (text: string) => void | Promise<void>;
  pollIntervalMs?: number;
  copyFeedbackMs?: number;
}

export const AgentAccessView: React.FC<AgentAccessViewProps> = ({
  client = defaultAgentAccessClient,
  writeClipboard,
  pollIntervalMs = POLL_INTERVAL_MS,
  copyFeedbackMs = COPY_FEEDBACK_MS,
}) => {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [live, setLive] = useState(true);
  const [past, setPast] = useState(true);
  const [reads, setReads] = useState<AgentReadAuditRow[]>([]);
  const [copied, setCopied] = useState(false);
  const [copyFallbackHint, setCopyFallbackHint] = useState(false);
  const [saveErrorSlot, setSaveErrorSlot] = useState<SaveErrorSlot>(null);
  const [saveErrorText, setSaveErrorText] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const applySnapshot = useCallback((snapshot: AgentAccessStateSnapshot) => {
    setEnabled(snapshot.enabled);
    setLive(snapshot.allow_live_meeting);
    setPast(snapshot.allow_past_meetings);
    setReads(
      snapshot.recent_reads
        .slice(0, MAX_VISIBLE_READS)
        .map(normalizeAgentReadRow),
    );
  }, []);

  const refreshState = useCallback(async () => {
    try {
      const res = await client.agentAccessState();
      if (mountedRef.current && res.status === "ok") {
        applySnapshot(res.data);
      }
    } catch {
      // Keep last known snapshot on transient poll error.
    }
  }, [applySnapshot, client]);

  useEffect(() => {
    mountedRef.current = true;
    void refreshState();

    const intervalId = setInterval(() => {
      if (
        typeof document === "undefined" ||
        document.visibilityState === "visible"
      ) {
        void refreshState();
      }
    }, pollIntervalMs);

    const onVisibilityChange = () => {
      if (
        typeof document !== "undefined" &&
        document.visibilityState === "visible"
      ) {
        void refreshState();
      }
    };
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    return () => {
      mountedRef.current = false;
      clearInterval(intervalId);
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
      if (copyTimerRef.current) {
        clearTimeout(copyTimerRef.current);
      }
    };
  }, [pollIntervalMs, refreshState]);

  const handleSave = useCallback(
    async (
      slot: Exclude<SaveErrorSlot, null>,
      saveFn: () => Promise<Result<AgentAccessStateSnapshot>>,
    ) => {
      setSaveErrorSlot(null);
      setSaveErrorText(null);
      const previous: AgentAccessStateSnapshot = {
        enabled,
        allow_live_meeting: live,
        allow_past_meetings: past,
        recent_reads: reads,
      };
      const outcome = await applyAgentAccessMutation(previous, saveFn);
      if (!mountedRef.current) return;
      applySnapshot(outcome.snapshot);
      if (outcome.failed) {
        setSaveErrorSlot(slot);
        setSaveErrorText(
          t("meetings.agent.saveError", "Couldn't save. Try again."),
        );
      }
    },
    [applySnapshot, enabled, live, past, reads, t],
  );

  const handleCopyCommand = useCallback(async () => {
    const { usedFallback } = await resolveAndCopyMcpCommand(
      client,
      async (text) => {
        if (writeClipboard) {
          await writeClipboard(text);
        } else {
          await navigator?.clipboard?.writeText?.(text);
        }
      },
    );

    if (!mountedRef.current) return;
    setCopyFallbackHint(usedFallback);
    setCopied(true);
    if (copyTimerRef.current) {
      clearTimeout(copyTimerRef.current);
    }
    copyTimerRef.current = setTimeout(() => {
      if (mountedRef.current) {
        setCopied(false);
      }
    }, copyFeedbackMs);
  }, [client, copyFeedbackMs, writeClipboard]);

  return (
    <AgentAccessSection
      enabled={enabled}
      allowLiveMeeting={live}
      allowPastMeetings={past}
      recentReads={reads}
      onToggleEnabled={(next) =>
        void handleSave("master", () => client.setAgentAccessEnabled(next))
      }
      onToggleLiveMeeting={(next) =>
        void handleSave("live", () => client.setAgentAccessScope(next, past))
      }
      onTogglePastMeetings={(next) =>
        void handleSave("past", () => client.setAgentAccessScope(live, next))
      }
      onCopyCommand={handleCopyCommand}
      copied={copied}
      copyFallbackHint={copyFallbackHint}
      saveErrorSlot={saveErrorSlot}
      saveErrorText={saveErrorText}
    />
  );
};
