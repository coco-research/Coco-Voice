import React from "react";
import { useTranslation } from "react-i18next";

const DOT_ON = "●";
const DOT_OFF = "○";
const DOT_WORKING = "◐";

export type StatusDotState = "on" | "off" | "working";

export interface StatusDotProps {
  state: StatusDotState;
  label: string;
  className?: string;
}

export const StatusDot: React.FC<StatusDotProps> = ({
  state,
  label,
  className = "",
}) => {
  const glyph =
    state === "on" ? DOT_ON : state === "working" ? DOT_WORKING : DOT_OFF;
  const tone =
    state === "on"
      ? "text-emerald-600 dark:text-emerald-400"
      : state === "working"
        ? "text-amber-600 dark:text-amber-400"
        : "text-mid-gray";

  return (
    <span
      data-status-dot={state}
      className={`inline-flex shrink-0 items-center gap-1.5 text-[12px] font-semibold ${tone} ${className}`}
    >
      <span aria-hidden="true">{glyph}</span>
      <span>{label}</span>
    </span>
  );
};

export interface PageHeaderProps {
  eyebrow: string;
  title: string;
  status?: { state: StatusDotState; label: string };
  action?: React.ReactNode;
  className?: string;
}

export const PageHeader: React.FC<PageHeaderProps> = ({
  eyebrow,
  title,
  status,
  action,
  className = "",
}) => (
  <header
    data-ui="page-header"
    className={`flex flex-wrap items-end justify-between gap-3 ${className}`}
  >
    <div className="min-w-0">
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {eyebrow}
      </p>
      <h1 className="mt-0.5 text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {title}
      </h1>
    </div>
    {status || action ? (
      <div className="flex flex-wrap items-center gap-2.5">
        {status ? (
          <StatusDot state={status.state} label={status.label} />
        ) : null}
        {action}
      </div>
    ) : null}
  </header>
);

export interface CardProps {
  children: React.ReactNode;
  ariaLabel?: string;
  className?: string;
  testId?: string;
}

export const Card: React.FC<CardProps> = ({
  children,
  ariaLabel,
  className = "",
  testId,
}) => (
  <section
    aria-label={ariaLabel}
    data-testid={testId}
    data-ui="card"
    className={`rounded-[14px] border border-mid-gray/25 bg-background p-4 ${className}`}
  >
    {children}
  </section>
);

export interface RowProps {
  label: string;
  subLabel?: string;
  value?: string;
  action?: React.ReactNode;
  className?: string;
}

export const Row: React.FC<RowProps> = ({
  label,
  subLabel,
  value,
  action,
  className = "",
}) => {
  const ariaParts = [label, subLabel, value].filter((part): part is string =>
    Boolean(part && part.trim().length > 0),
  );

  return (
    <div
      data-ui="row"
      aria-label={ariaParts.join(", ")}
      className={`flex flex-wrap items-center justify-between gap-3 py-2.5 ${className}`}
    >
      <div className="min-w-0">
        <div className="text-[13.5px] font-semibold text-text">{label}</div>
        {subLabel ? (
          <div className="text-[12.5px] text-mid-gray">{subLabel}</div>
        ) : null}
      </div>
      {value || action ? (
        <div className="flex shrink-0 items-center gap-2.5">
          {value ? (
            <span className="text-[12.5px] font-medium text-mid-gray">
              {value}
            </span>
          ) : null}
          {action}
        </div>
      ) : null}
    </div>
  );
};

export interface SegmentedOption<T extends string = string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SegmentedProps<T extends string = string> {
  ariaLabel: string;
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}

export function nextSegmentedIndex<T extends string>(
  options: readonly SegmentedOption<T>[],
  currentIndex: number,
  key: string,
): number {
  const len = options.length;
  if (len === 0) return -1;
  if (key === "Home") {
    return options.findIndex((o) => !o.disabled);
  }
  if (key === "End") {
    for (let i = len - 1; i >= 0; i -= 1) {
      if (!options[i]?.disabled) return i;
    }
    return currentIndex;
  }
  const step = key === "ArrowRight" || key === "ArrowDown" ? 1 : -1;
  for (let offset = 1; offset <= len; offset += 1) {
    const idx = (currentIndex + step * offset + len) % len;
    if (!options[idx]?.disabled) return idx;
  }
  return currentIndex;
}

export function Segmented<T extends string = string>({
  ariaLabel,
  options,
  value,
  onChange,
  className = "",
}: SegmentedProps<T>) {
  const { t } = useTranslation();
  const total = options.length;

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      data-ui="segmented"
      className={`inline-flex flex-wrap items-center gap-1 rounded-[10px] border border-mid-gray/30 bg-mid-gray/[0.06] p-1 ${className}`}
    >
      {options.map((opt, idx) => {
        const selected = opt.value === value;
        const positionLabel = selected
          ? t("ui.segmented.optionSelected", {
              label: opt.label,
              index: idx + 1,
              total,
              defaultValue: "{{label}}, {{index}} of {{total}}, selected",
            })
          : t("ui.segmented.option", {
              label: opt.label,
              index: idx + 1,
              total,
              defaultValue: "{{label}}, {{index}} of {{total}}",
            });

        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={positionLabel}
            tabIndex={selected ? 0 : -1}
            disabled={opt.disabled}
            onClick={() => onChange(opt.value)}
            onKeyDown={(e) => {
              if (
                e.key === "ArrowRight" ||
                e.key === "ArrowLeft" ||
                e.key === "ArrowUp" ||
                e.key === "ArrowDown" ||
                e.key === "Home" ||
                e.key === "End"
              ) {
                e.preventDefault();
                const nextIdx = nextSegmentedIndex(options, idx, e.key);
                const nextOpt = options[nextIdx];
                if (nextOpt && !nextOpt.disabled) {
                  onChange(nextOpt.value);
                }
              }
            }}
            className={`rounded-[7px] px-2.5 py-1 text-[12.5px] font-semibold transition-colors cursor-pointer disabled:opacity-45 ${
              selected
                ? "bg-logo-primary text-white"
                : "text-text hover:bg-mid-gray/15"
            }`}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

export function formatTimeChipLabel(totalSeconds: number): {
  display: string;
  minutes: number;
  seconds: number;
} {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(safe / 60);
  const seconds = safe % 60;
  const display = `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  return { display, minutes, seconds };
}

export interface TimeChipProps {
  seconds: number;
  onJump: (seconds: number) => void;
  className?: string;
}

export const TimeChip: React.FC<TimeChipProps> = ({
  seconds,
  onJump,
  className = "",
}) => {
  const { t } = useTranslation();
  const formatted = formatTimeChipLabel(seconds);

  return (
    <button
      type="button"
      data-ui="time-chip"
      onClick={() => onJump(seconds)}
      aria-label={t("ui.timeChip.jumpTo", {
        minutes: formatted.minutes,
        seconds: formatted.seconds,
        defaultValue: "Jump to {{minutes}} minutes {{seconds}} seconds",
      })}
      className={`inline-flex shrink-0 items-center rounded-[6px] border border-mid-gray/35 bg-mid-gray/[0.06] px-2 py-0.5 font-mono text-[11.5px] font-semibold text-text cursor-pointer hover:border-logo-primary hover:text-logo-primary ${className}`}
    >
      {formatted.display}
    </button>
  );
};

export interface NoticeProps {
  title: string;
  body: string;
  actionLabel: string;
  onAction: () => void;
  accent?: boolean;
  testId?: string;
  className?: string;
}

export const Notice: React.FC<NoticeProps> = ({
  title,
  body,
  actionLabel,
  onAction,
  accent = false,
  testId,
  className = "",
}) => (
  <section
    role="status"
    data-ui="notice"
    data-testid={testId}
    className={`rounded-[14px] bg-background p-4 space-y-2 ${
      accent
        ? "border-[1.5px] border-logo-primary"
        : "border border-mid-gray/35"
    } ${className}`}
  >
    <h3 className="text-[15px] font-semibold text-text">{title}</h3>
    <p className="text-[13px] leading-[19px] text-mid-gray">{body}</p>
    <div className="pt-0.5">
      <button
        type="button"
        onClick={onAction}
        className={
          accent
            ? "inline-flex min-h-[30px] items-center rounded-[9px] bg-logo-primary px-3 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90"
            : "inline-flex min-h-[30px] items-center rounded-[8px] border border-mid-gray/45 bg-background px-3 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        }
      >
        {actionLabel}
      </button>
    </div>
  </section>
);
