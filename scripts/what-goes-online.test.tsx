import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { loadAllowlist, type PrivacyAllowEntry } from "./privacy-scan.ts";
import {
  ONLINE_CLOUD_PROVIDERS,
  ONLINE_CUSTOM_HOST,
  ONLINE_EXCLUDED_WHEN,
  ONLINE_WHEN_BY_GROUP,
  WhatGoesOnline,
  WhatGoesOnlineSection,
} from "../src/components/settings/about/WhatGoesOnline";

const EXCLUDED_WHEN = new Set<string>([
  "not contacted by the app",
  "only when a Windows build signs the installer",
]);

export function partitionRuntimeAllowlist(
  entries: readonly PrivacyAllowEntry[],
) {
  const out = {
    open: [] as string[],
    click: [] as string[],
    cleanup: [] as string[],
  };
  const byWhen = new Map<string, keyof typeof out>(
    Object.entries(ONLINE_WHEN_BY_GROUP).map(([g, w]) => [
      w,
      g as keyof typeof out,
    ]),
  );
  for (const e of entries) {
    if (EXCLUDED_WHEN.has(e.when)) continue;
    const g = byWhen.get(e.when);
    if (!g)
      throw new Error(
        `Unknown privacy-allowlist when value for ${e.host}: "${e.when}"`,
      );
    out[g].push(e.host);
  }
  return {
    open: out.open.sort(),
    click: out.click.sort(),
    cleanup: out.cleanup.sort(),
  };
}

function makeTestI18n() {
  const inst = i18next.createInstance();
  void inst.use(initReactI18next).init({
    lng: "en",
    fallbackLng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function renderOnline(updateChecksEnabled = true): string {
  return renderToStaticMarkup(
    <I18nextProvider i18n={makeTestI18n()}>
      <WhatGoesOnlineSection updateChecksEnabled={updateChecksEnabled} />
    </I18nextProvider>,
  ).replaceAll("&#x27;", "'");
}

function renderForOs(osType: string): string {
  const g = globalThis as { window?: unknown };
  const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: osType } };
  try {
    return renderToStaticMarkup(
      <I18nextProvider i18n={makeTestI18n()}>
        <WhatGoesOnline />
      </I18nextProvider>,
    );
  } finally {
    if (prev) {
      Object.defineProperty(globalThis, "window", prev);
    } else {
      delete g.window;
    }
  }
}

const hostsIn = (s: string) =>
  [
    ...s.matchAll(/<code[^>]*data-online-host="([^"]+)"[^>]*>([^<]+)<\/code>/g),
  ].map((m) => {
    expect(m[2]).toBe(m[1]);
    return m[1];
  });

describe("WhatGoesOnlineSection", () => {
  test("rendered hosts equal privacy-allowlist minus the two non-runtime when strings, with no extra hosts", () => {
    expect([...ONLINE_EXCLUDED_WHEN].sort()).toEqual([...EXCLUDED_WHEN].sort());
    const allowlist = loadAllowlist();
    const byGroup = partitionRuntimeAllowlist(allowlist);
    const expected = [
      ...byGroup.open,
      ...byGroup.click,
      ...byGroup.cleanup,
    ].sort();
    expect(expected.length).toBe(11);

    const html = renderOnline(true);
    const rendered = hostsIn(html);
    expect(new Set(rendered).size).toBe(rendered.length);
    expect([...rendered].sort()).toEqual(expected);
    for (const g of ["open", "click", "cleanup"] as const) {
      const i = html.indexOf(`data-online-group="${g}"`);
      const j = html.indexOf('data-online-group="', i + 1);
      expect(
        hostsIn(j === -1 ? html.slice(i) : html.slice(i, j)).sort(),
      ).toEqual(byGroup[g]);
    }
    const domains = [
      ...html
        .replace(/<[^>]+>/g, " ")
        .matchAll(/\b(?:localhost|[a-z0-9-]+(?:\.[a-z0-9-]+)+)\b/g),
    ].map((m) => m[0]);
    expect([...new Set(domains)].sort()).toEqual(expected);
    for (const e of allowlist.filter((x) => EXCLUDED_WHEN.has(x.when))) {
      expect(html).not.toContain(e.host);
    }
  });

  test("fails when an allowlist entry uses an unknown when string", () => {
    expect(() =>
      partitionRuntimeAllowlist([
        { host: "x.example", why: "x", when: "never", sends: "x", files: [] },
      ]),
    ).toThrow(/Unknown privacy-allowlist when value/);
  });

  test("groups the 7 cloud clean-up providers under one shared When and Leaves block while keeping Custom separate", () => {
    const html = renderOnline(true);
    expect(html.split(en.settings.online.cloudWhen).length - 1).toBe(1);
    expect(html.split(en.settings.online.cloudLeaves).length - 1).toBe(1);
    const cloud =
      html.match(
        /<div data-online-cloud-group="true"[^>]*>([\s\S]*?)<\/div><div class="flex/,
      )?.[1] ?? "";
    const rows = [
      ...cloud.matchAll(
        /<div data-online-cloud-row="([^"]+)"[^>]*>([\s\S]*?)<\/div>/g,
      ),
    ];
    expect(rows.map((m) => m[1])).toEqual(
      ONLINE_CLOUD_PROVIDERS.map((p) => p.host),
    );
    for (const [, host, inner] of rows) {
      expect(host).not.toBe(ONLINE_CUSTOM_HOST);
      expect(inner).not.toContain("<p");
    }
    expect(cloud).not.toContain(ONLINE_CUSTOM_HOST);
    expect(html).toContain(`>${en.settings.online.eyebrow}<`);
    expect(html).toContain("Custom clean-up (Ollama)");
    expect(html).toContain("Stays on this Mac");
    expect(html).toContain("Update checks on");
    expect(html).toContain(en.settings.online.onDeviceNote);
    expect(renderOnline(false)).toContain("Update checks off");

    const localesDir = join(import.meta.dir, "../src/i18n/locales");
    for (const loc of readdirSync(localesDir).sort()) {
      const raw = JSON.parse(
        readFileSync(join(localesDir, loc, "translation.json"), "utf8"),
      ) as { settings?: { online?: { eyebrow?: string } } };
      const eyebrow = raw.settings?.online?.eyebrow ?? "";
      expect(eyebrow.length).toBeGreaterThan(0);
      expect(eyebrow).not.toMatch(/[.…]/);
    }
  });

  test("renders nothing when the OS is not macOS", () => {
    expect(renderForOs("windows")).toBe("");
    expect(renderForOs("linux")).toBe("");
    expect(renderForOs("macos")).toContain('data-testid="what-goes-online"');
  });
});
