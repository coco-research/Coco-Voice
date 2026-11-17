import { useTranslation } from "react-i18next";
import { Check, Loader2 } from "lucide-react";

export type PermissionStatus =
  "checking" | "needed" | "waiting" | "granted" | "error";

interface PermissionActionProps {
  status: PermissionStatus;
  grantLabel: string;
  onGrant: () => void;
  onRetry: () => void;
}

const buttonClass =
  "px-4 py-2 rounded-lg bg-logo-primary hover:bg-logo-primary/90 text-white text-sm font-medium transition-colors";

/** The status line or button under one permission card in onboarding. */
const PermissionAction: React.FC<PermissionActionProps> = ({
  status,
  grantLabel,
  onGrant,
  onRetry,
}) => {
  const { t } = useTranslation();

  if (status === "granted") {
    return (
      <div className="flex items-center gap-2 text-emerald-400 text-sm">
        <Check className="w-4 h-4" />
        {t("onboarding.permissions.granted")}
      </div>
    );
  }

  if (status === "waiting") {
    return (
      <div className="flex items-center gap-2 text-text/50 text-sm">
        <Loader2 className="w-4 h-4 animate-spin" />
        {t("onboarding.permissions.waiting")}
      </div>
    );
  }

  if (status === "error") {
    return (
      <div className="flex flex-col items-start gap-2">
        <p className="text-sm text-red-400">
          {t("onboarding.permissions.errors.checkFailed")}
        </p>
        <div className="flex flex-wrap gap-2">
          <button onClick={onRetry} className={buttonClass}>
            {t("errors.shortcutRegistrationTryAgain")}
          </button>
          <button onClick={onGrant} className={buttonClass}>
            {grantLabel}
          </button>
        </div>
      </div>
    );
  }

  return (
    <button onClick={onGrant} className={buttonClass}>
      {grantLabel}
    </button>
  );
};

export default PermissionAction;
