import React from "react";
import ReactDOM from "react-dom/client";
import "./RecordingOverlay.css";
import RecordingOverlay from "./RecordingOverlay";
import { RecordingPillShowcase } from "./RecordingPillStates";
import "@/i18n";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";

if (typeof window !== "undefined") {
  applyTheme(getStoredTheme());
  void syncThemeFromSettings();
}

const isShowcase =
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).get("showcase") === "all-states";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isShowcase ? <RecordingPillShowcase /> : <RecordingOverlay />}
  </React.StrictMode>,
);
