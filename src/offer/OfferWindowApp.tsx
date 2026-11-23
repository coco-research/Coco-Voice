import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import "@/i18n";
import { syncLanguageFromSettings } from "@/i18n";
import {
  THEME_STORAGE_KEY,
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import { useSettings } from "@/hooks/useSettings";
import {
  commands,
  type MeetingOffer,
  type MeetingStatus,
} from "../bindings.meetings";
import { OfferCard } from "../components/meetings/OfferCard";
import { sidebarVisible } from "../components/meetings/meetings-ui";
import {
  INITIAL_OFFER_VIEW_STATE,
  applyMeetingStatus,
  applyOfferEvent,
  isOfferVisible,
  markOfferHeld,
} from "../components/meetings/offerState";

function OfferWindowRoot() {
  const { settings, refreshSettings } = useSettings();
  const [viewState, setViewState] = useState(INITIAL_OFFER_VIEW_STATE);
  const sawOfferEvent = useRef(false);
  const sawStateEvent = useRef(false);

  useEffect(() => {
    applyTheme(getStoredTheme());
    void syncThemeFromSettings();
    void syncLanguageFromSettings();

    const onStorage = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY) {
        applyTheme(getStoredTheme());
      }
    };
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  useEffect(() => {
    if (settings?.theme) {
      applyTheme(settings.theme);
    }
  }, [settings?.theme]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    const onOfferPayload = (payload: MeetingOffer | null) => {
      sawOfferEvent.current = true;
      void refreshSettings();
      void syncThemeFromSettings();
      void syncLanguageFromSettings();
      setViewState((prev) => applyOfferEvent(prev, payload));
    };

    const onStatusPayload = (payload: MeetingStatus) => {
      sawStateEvent.current = true;
      setViewState((prev) => applyMeetingStatus(prev, payload.state));
    };

    void listen<MeetingOffer | null>("meetings-offer", (event) => {
      onOfferPayload(event.payload);
    }).then((stop) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    });

    void listen<MeetingStatus>("meeting-state-changed", (event) => {
      if (event.payload) onStatusPayload(event.payload);
    }).then((stop) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    });

    commands
      .currentMeetingOffer()
      .then((result) => {
        if (
          cancelled ||
          sawOfferEvent.current ||
          result.status === "error"
        ) {
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
  }, [refreshSettings]);

  const visible = isOfferVisible(viewState, sidebarVisible(settings));

  useEffect(() => {
    if (visible) {
      void syncThemeFromSettings();
    }
    void commands.setMeetingOfferVisible(visible).catch((error: unknown) => {
      console.error("Failed to update meeting offer window visibility", error);
    });
  }, [visible]);

  if (!visible || !viewState.offer) {
    return null;
  }

  const activeOffer = viewState.offer;

  const handleStart = () => {
    setViewState((prev) => markOfferHeld(prev));
    void commands.setMeetingOfferVisible(false);
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

  const handleNotNow = () => {
    const bundleId = activeOffer.bundleId;
    sawOfferEvent.current = true;
    setViewState(INITIAL_OFFER_VIEW_STATE);
    void commands.setMeetingOfferVisible(false);
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
    <div className="offer-window-frame">
      <OfferCard
        offer={activeOffer}
        onStart={handleStart}
        onNotNow={handleNotNow}
      />
    </div>
  );
}

export function mountOfferWindow() {
  const root = document.getElementById("root");
  if (!root) return;
  applyTheme(getStoredTheme());
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <OfferWindowRoot />
    </React.StrictMode>,
  );
}
