import { useEffect, useState, type ComponentType } from "react";
import type {
  MeetingSegmentView,
  MeetingSpeakerSlot,
} from "../../bindings.meetings";

export const SPEAKER_SPLIT_ENABLED = import.meta.env.VITE_SPEAKER_SPLIT === "1";

export interface SpeakerSplitSectionProps {
  meetingId: number;
  segments: MeetingSegmentView[];
  initialSlots?: MeetingSpeakerSlot[];
  working?: boolean;
  modelsMissing?: boolean;
  onDownloadModels?: () => void;
  onSlotsLoaded?: (slots: MeetingSpeakerSlot[]) => void;
  onRenamed?: (slots: MeetingSpeakerSlot[]) => void;
}

export function buildSpeakerNameMapFromSlots(
  slots: MeetingSpeakerSlot[],
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of slots) {
    const name = (row.custom_name || row.display_name || "").trim();
    if (name) {
      out[row.slot] = name;
    }
  }
  return out;
}

// Dynamic import stays strictly inside the env check so Vite deletes the
// speaker-split chunk and all its strings when VITE_SPEAKER_SPLIT !== "1" (AC-X41).
export function useSpeakerSplitSection(): ComponentType<SpeakerSplitSectionProps> | null {
  const [Comp, setComp] =
    useState<ComponentType<SpeakerSplitSectionProps> | null>(null);

  useEffect(() => {
    if (import.meta.env.VITE_SPEAKER_SPLIT !== "1") return;
    let cancelled = false;
    import("./SpeakerSplitSection").then((mod) => {
      if (!cancelled) setComp(() => mod.SpeakerSplitSection);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return Comp;
}

export interface MeetingsUi {
  MEETINGS_UI_SENTINEL: string;
  MeetingsToggle: ComponentType<{
    descriptionMode?: "inline" | "tooltip";
    grouped?: boolean;
  }>;
  ConsentDialog: ComponentType;
  MeetingsView: ComponentType;
  OfferPanel: ComponentType;
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
