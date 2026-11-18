import { useEffect, useState, type ComponentType } from "react";

export interface MeetingsUi {
  MEETINGS_UI_SENTINEL: string;
  MeetingsToggle: ComponentType<{
    descriptionMode?: "inline" | "tooltip";
    grouped?: boolean;
  }>;
  ConsentDialog: ComponentType;
  MeetingsView: ComponentType;
  icon: ComponentType<{
    width?: number | string;
    height?: number | string;
    className?: string;
  }>;
  sidebarVisible: (settings: unknown) => boolean;
}

// The dynamic import stays inside the env check so Vite deletes it, and the
// meetings chunk, when VITE_MEETINGS is not "1".
export function useMeetingsUi(): MeetingsUi | null {
  const [ui, setUi] = useState<MeetingsUi | null>(null);

  useEffect(() => {
    if (import.meta.env.VITE_MEETINGS !== "1") return;
    let cancelled = false;
    import("./meetings-ui").then((mod) => {
      if (!cancelled) setUi(mod.meetingsUi);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return ui;
}
