import { expect, mock, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

// Translation keys stand in for the strings, so the markup names what it shows.
mock.module("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { accessibilityStatus } =
  await import("../src/lib/utils/accessibilityStatus");
const { default: AccessibilityStaleNotice } =
  await import("../src/components/onboarding/AccessibilityStaleNotice");

const STALE = "onboarding.permissions.accessibility.stale";

test("trusted with a failed input probe is a stale entry", () => {
  expect(accessibilityStatus(true, "failed")).toBe("stale");
});

test("trusted with a working input probe is granted", () => {
  expect(accessibilityStatus(true, "ok")).toBe("granted");
});

test("an untrusted app is not granted, whatever the probe says", () => {
  expect(accessibilityStatus(false, "not-run")).toBe("needed");
  expect(accessibilityStatus(false, "failed")).toBe("needed");
  expect(accessibilityStatus(false, "ok")).toBe("needed");
});

test("a trusted app whose probe didn't run is not granted", () => {
  expect(accessibilityStatus(true, "not-run")).toBe("needed");
});

test("the stale notice shows the remove-and-re-add steps and Check again", () => {
  const html = renderToStaticMarkup(
    createElement(AccessibilityStaleNotice, { onCheckAgain: () => {} }),
  );
  for (const key of ["description", "step1", "step2", "step3", "checkAgain"]) {
    expect(html).toContain(`${STALE}.${key}`);
  }
  expect(html).toContain("<button");
  expect(html).toContain("<ol");
});
