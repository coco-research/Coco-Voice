import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Users } from "lucide-react";
import { platform, version } from "@tauri-apps/plugin-os";
import {
  commands,
  type MeetingOffer,
  type MeetingStatus,
  type Result,
} from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import { MeetingsToggle } from "../settings/MeetingsToggle";
import { OfferCard } from "./OfferCard";
import { ConsentDialog as ApprovedConsentDialog } from "./MeetingEntryPanels";
import {
  MEETINGS_OFFER_EVENT,
  MEETINGS_OPEN_CONSENT_EVENT,
  MEETING_STATE_EVENT,
  handOffMeetingStartRefusal,
} from "./meetingEvents";
import type { MeetingsUi } from "./gate";
import {
  INITIAL_OFFER_VIEW_STATE,
  applyMeetingStatus,
  applyOfferEvent,
  isOfferVisible,
  markOfferHeld,
} from "./offerState";
import { RecordMeetingPage as MeetingsView } from "./RecordMeetingPage";
export const MEETINGS_UI_SENTINEL = "coco-meetings-ui-v1";

export type OptimisticActionOutcome<T> = { ok: true; data: T } | { ok: false };

export async function runOptimisticCommand<T, E = string>(
  apply: () => void,
  run: () => Promise<Result<T, E>>,
  undo: () => void,
): Promise<OptimisticActionOutcome<T>> {
  apply();
  try {
    const res = await run();
    if (res.status === "ok") {
      return { ok: true, data: res.data };
    }
  } catch {
    // Undo optimistic update below on unexpected rejection.
  }
  undo();
  return { ok: false };
}

function meetingsEnabled(settings: unknown): boolean {
  return (
    typeof settings === "object" &&
    settings !== null &&
    "meetings_enabled" in settings &&
    settings.meetings_enabled === true
  );
}

// Same major.minor rule as `macos_at_least_14_4` in src-tauri/src/settings.rs.
function macosAtLeast14_4(os: string, osVersion: string): boolean {
  if (os !== "macos") return false;
  const [majorRaw, minorRaw] = osVersion.split(".");
  const major = Number.parseInt(majorRaw ?? "", 10);
  if (!Number.isFinite(major)) return false;
  const minor = minorRaw === undefined ? 0 : Number.parseInt(minorRaw, 10);
  if (!Number.isFinite(minor)) return false;
  return major > 14 || (major === 14 && minor >= 4);
}

export function sidebarVisible(settings: unknown): boolean {
  return meetingsEnabled(settings) && macosAtLeast14_4(platform(), version());
}

// Mock 2. The dialog only asks; Start is a separate command and Cancel stores
// nothing. "I've told everyone" is recommended, not a gate, and is never
// remembered: every open starts with it unchecked.
function ConsentDialogPanel({ onClose }: { onClose: () => void }) {
  const [starting, setStarting] = useState(false);

  const start = async () => {
    setStarting(true);
    const result = await commands.confirmMeetingStart();
    setStarting(false);
    if (result.status === "error") {
      onClose();
      handOffMeetingStartRefusal(result.error ?? "system_audio_unavailable");
      return;
    }
    onClose();
  };

  const cancel = async () => {
    await commands.cancelMeetingConsent();
    onClose();
  };

  return (
    <ApprovedConsentDialog
      open={true}
      starting={starting}
      errorMessage={null}
      onConfirmStart={() => void start()}
      onCancel={() => void cancel()}
    />
  );
}

// Mounted app-wide (App.tsx) so the shortcut and the offer open the dialog from
// any section, not only while the Meetings view is on screen.
export function ConsentDialog() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(MEETINGS_OPEN_CONSENT_EVENT, () => setOpen(true)).then(
      (stop) => {
        if (disposed) stop();
        else unlisten = stop;
      },
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!open) return null;
  return <ConsentDialogPanel onClose={() => setOpen(false)} />;
}

export function OfferPanel() {
  const { settings } = useSettings();
  const [viewState, setViewState] = useState(INITIAL_OFFER_VIEW_STATE);
  const sawOfferEvent = useRef(false);
  const sawStateEvent = useRef(false);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    void listen<MeetingOffer | null>(MEETINGS_OFFER_EVENT, (event) => {
      sawOfferEvent.current = true;
      setViewState((prev) => applyOfferEvent(prev, event.payload));
    }).then((stop) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    });
    void listen<MeetingStatus>(MEETING_STATE_EVENT, (event) => {
      if (!event.payload) return;
      sawStateEvent.current = true;
      setViewState((prev) => applyMeetingStatus(prev, event.payload.state));
    }).then((stop) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    });
    commands
      .currentMeetingOffer()
      .then((result) => {
        if (cancelled || sawOfferEvent.current || result.status === "error") {
          return;
        }
        setViewState((prev) => applyOfferEvent(prev, result.data));
      })
      .catch((error: unknown) => {
        console.error("Failed to read the meeting offer", error);
      });
    commands
      .meetingStatus()
      .then((status) => {
        if (cancelled || sawStateEvent.current) return;
        setViewState((prev) => applyMeetingStatus(prev, status.state));
      })
      .catch((error: unknown) => {
        console.error("Failed to read the meeting status", error);
      });
    return () => {
      cancelled = true;
      for (const stop of unlisteners) stop();
    };
  }, []);

  if (
    !isOfferVisible(viewState, sidebarVisible(settings)) ||
    !viewState.offer
  ) {
    return null;
  }

  const activeOffer = viewState.offer;

  const onStart = () => {
    setViewState((prev) => markOfferHeld(prev));
    void commands.openMeetingConsent().then(
      (result) => {
        if (result.status === "error") {
          setViewState((prev) => applyMeetingStatus(prev, "idle"));
        }
      },
      (error: unknown) => {
        console.error("Failed to open the meeting consent reminder", error);
        setViewState((prev) => applyMeetingStatus(prev, "idle"));
      },
    );
  };

  const onNotNow = () => {
    const bundleId = activeOffer.bundleId;
    sawOfferEvent.current = true;
    setViewState(INITIAL_OFFER_VIEW_STATE);
    void commands.dismissMeetingOffer(bundleId).then(
      (result) => {
        if (result.status === "error") {
          console.error("Failed to dismiss the meeting offer", result.error);
        }
      },
      (error: unknown) => {
        console.error("Failed to dismiss the meeting offer", error);
      },
    );
  };

  return (
    <OfferCard offer={activeOffer} onStart={onStart} onNotNow={onNotNow} />
  );
}

export const meetingsUi = {
  MEETINGS_UI_SENTINEL,
  MeetingsToggle,
  ConsentDialog,
  MeetingsView,
  OfferPanel,
  icon: Users,
  sidebarVisible,
} satisfies MeetingsUi;
