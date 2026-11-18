import React, { useId } from "react";
import { useTranslation } from "react-i18next";
import type { MeasuredReason } from "@/bindings";
import Badge from "../ui/Badge";

interface RecommendedBadgeProps {
  reason: MeasuredReason;
}

/**
 * The "Recommended" tag. It only exists for a model with a measured reason; the
 * reason shows on hover and on keyboard focus, and is tied to the tag with
 * aria-describedby so screen readers announce it.
 */
const RecommendedBadge: React.FC<RecommendedBadgeProps> = ({ reason }) => {
  const { t } = useTranslation();
  const tooltipId = useId();
  // The tag sits inside a clickable card; keep its own key and click presses
  // from selecting or downloading the model.
  const stop = (e: React.SyntheticEvent) => e.stopPropagation();

  return (
    <span
      className="relative inline-flex group/recommended"
      tabIndex={0}
      aria-describedby={tooltipId}
      data-testid="recommended-badge"
      onClick={stop}
      onKeyDown={stop}
    >
      <Badge variant="primary">{t("onboarding.recommended")}</Badge>
      <span
        id={tooltipId}
        role="tooltip"
        className="pointer-events-none absolute left-0 top-full z-20 mt-2 w-64 rounded-md border border-mid-gray/30 bg-background px-3 py-2 text-xs font-normal text-text/80 shadow-lg opacity-0 transition-opacity group-hover/recommended:opacity-100 group-focus/recommended:opacity-100"
      >
        {t("onboarding.measuredReason", {
          wrong: reason.words_wrong_per_100,
          speed: reason.speed_x_realtime,
          memory: reason.peak_memory_mb,
          compared: reason.models_compared,
        })}
      </span>
    </span>
  );
};

export default RecommendedBadge;
