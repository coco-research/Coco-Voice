import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import i18next from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import type { MeetingOffer } from "../src/bindings.meetings";
import en from "../src/i18n/locales/en/translation.json";
import { OfferCard } from "../src/components/meetings/OfferCard";
import { AgentsCanAskSection } from "../src/components/settings/AgentsCanAskCard";
import {
  INITIAL_OFFER_VIEW_STATE,
  applyMeetingStatus,
  applyOfferEvent,
  isOfferVisible,
  markOfferHeld,
  shouldShowOffer,
} from "../src/components/meetings/offerState";

const root = join(import.meta.dir, "..");

async function createTestI18n() {
  const instance = i18next.createInstance();
  await instance.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return instance;
}

const ZOOM_CALL: MeetingOffer = {
  bundleId: "us.zoom.xos",
  app: "zoom",
  inCall: true,
  startsCapture: false,
};

const ZOOM_OPEN: MeetingOffer = {
  bundleId: "us.zoom.xos",
  app: "zoom",
  inCall: false,
  startsCapture: false,
};

const TEAMS_CALL: MeetingOffer = {
  bundleId: "com.microsoft.teams2",
  app: "teams",
  inCall: true,
  startsCapture: false,
};

const TEAMS_OPEN: MeetingOffer = {
  bundleId: "com.microsoft.teams2",
  app: "teams",
  inCall: false,
  startsCapture: false,
};

const MEET_TAB: MeetingOffer = {
  bundleId: "com.google.Chrome",
  app: "meet",
  inCall: false,
  startsCapture: false,
};

describe("meeting offer floating panel and wired v0.20 screens", () => {
  test("shows only active calls and Meet browser call tabs", () => {
    expect(shouldShowOffer(null)).toBe(false);
    expect(shouldShowOffer(ZOOM_OPEN)).toBe(false);
    expect(shouldShowOffer(TEAMS_OPEN)).toBe(false);
    expect(shouldShowOffer(ZOOM_CALL)).toBe(true);
    expect(shouldShowOffer(TEAMS_CALL)).toBe(true);
    expect(shouldShowOffer(MEET_TAB)).toBe(true);
  });

  test("renders brand row, copy, actions, and footer after buttons", async () => {
    const i18n = await createTestI18n();
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <OfferCard offer={ZOOM_CALL} onStart={() => {}} onNotNow={() => {}} />
      </I18nextProvider>,
    );

    expect(html).toContain("Coco Voice");
    expect(html).toContain("radialGradient");
    expect(html).toContain("Start notes?");
    expect(html).toContain("Zoom is in a call.");
    expect(html).toContain("Not recording");
    expect(html).toContain("Start notes");
    expect(html).toContain("Not now");
    expect(html).toContain("Coco Voice never records on its own.");

    const brandIndex = html.indexOf("Coco Voice");
    const headingIndex = html.indexOf("Start notes?");
    const startIndex = html.indexOf(">Start notes<");
    const notNowIndex = html.indexOf(">Not now<");
    const footerIndex = html.indexOf("Coco Voice never records on its own.");

    expect(brandIndex).toBeGreaterThanOrEqual(0);
    expect(headingIndex).toBeGreaterThan(brandIndex);
    expect(startIndex).toBeGreaterThan(headingIndex);
    expect(notNowIndex).toBeGreaterThan(startIndex);
    expect(footerIndex).toBeGreaterThan(notNowIndex);
  });

  test("clears held state when consent reminder is cancelled or offer changes", () => {
    const shown = applyOfferEvent(INITIAL_OFFER_VIEW_STATE, ZOOM_CALL);
    expect(isOfferVisible(shown, true)).toBe(true);

    const held = markOfferHeld(shown);
    expect(held.held).toBe(true);
    expect(isOfferVisible(held, true)).toBe(false);

    const unchanged = applyOfferEvent(held, { ...ZOOM_CALL });
    expect(unchanged.held).toBe(true);
    expect(isOfferVisible(unchanged, true)).toBe(false);

    const afterCancel = applyMeetingStatus(held, "idle");
    expect(afterCancel.held).toBe(false);
    expect(isOfferVisible(afterCancel, true)).toBe(true);

    const heldAgain = markOfferHeld(afterCancel);
    const changedOffer = applyOfferEvent(heldAgain, TEAMS_CALL);
    expect(changedOffer.held).toBe(false);
    expect(changedOffer.offer).toEqual(TEAMS_CALL);
    expect(isOfferVisible(changedOffer, true)).toBe(true);

    const afterIdleApp = applyOfferEvent(heldAgain, ZOOM_OPEN);
    expect(afterIdleApp).toEqual({ offer: null, held: false });
  });

  test("renders Agents can ask you (V-D18 B29/B30) in off and on states", async () => {
    const i18n = await createTestI18n();
    const offHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <AgentsCanAskSection enabled={false} onToggleEnabled={() => {}} />
      </I18nextProvider>,
    );
    expect(offHtml).toContain("agents-can-ask-state-off");
    expect(offHtml).toContain("coco-voice --mcp-ask-user");

    const onHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <AgentsCanAskSection enabled={true} onToggleEnabled={() => {}} />
      </I18nextProvider>,
    );
    expect(onHtml).toContain("agents-can-ask-state-on");
  });

  test("mounts offer window entrypoint, Rust NSPanel, and wired UI surfaces", () => {
    const appTsx = readFileSync(join(root, "src/App.tsx"), "utf8");
    expect(appTsx).not.toContain("meetingsUi.OfferPanel");
    expect(appTsx).toContain("meetingsUi.ConsentDialog");
    expect(appTsx).toContain("appVersion={appVersion}");
    expect(appTsx).toContain("getVersion");

    const viteConfig = readFileSync(join(root, "vite.config.ts"), "utf8");
    expect(viteConfig).toContain("src/offer/index.html");

    const capabilities = JSON.parse(
      readFileSync(join(root, "src-tauri/capabilities/default.json"), "utf8"),
    );
    expect(capabilities.windows).toContain("meeting_offer");

    const libRs = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    expect(libRs).toContain("mod meeting_offer_panel;");
    expect(libRs).toContain("set_meeting_offer_visible");

    const overlayTsx = readFileSync(
      join(root, "src/overlay/RecordingOverlay.tsx"),
      "utf8",
    );
    expect(overlayTsx).toContain("RecordingPillStateView");
  });
});
