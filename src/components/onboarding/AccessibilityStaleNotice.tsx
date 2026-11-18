import { useTranslation } from "react-i18next";

interface AccessibilityStaleNoticeProps {
  onCheckAgain: () => void;
}

const STEPS = ["step1", "step2", "step3"] as const;

/** Shown when macOS lists Coco Voice as allowed but input still fails. */
const AccessibilityStaleNotice: React.FC<AccessibilityStaleNoticeProps> = ({
  onCheckAgain,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col items-start gap-2">
      <p className="text-sm text-amber-400">
        {t("onboarding.permissions.accessibility.stale.description")}
      </p>
      <ol className="list-decimal ps-5 text-sm text-text/70 space-y-1">
        {STEPS.map((step) => (
          <li key={step}>
            {t(`onboarding.permissions.accessibility.stale.${step}`)}
          </li>
        ))}
      </ol>
      <button
        onClick={onCheckAgain}
        className="px-4 py-2 rounded-lg bg-logo-primary hover:bg-logo-primary/90 text-white text-sm font-medium transition-colors"
      >
        {t("onboarding.permissions.accessibility.stale.checkAgain")}
      </button>
    </div>
  );
};

export default AccessibilityStaleNotice;
