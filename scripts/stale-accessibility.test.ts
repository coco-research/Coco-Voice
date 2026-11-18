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

const { default: PermissionAction } =
  await import("../src/components/onboarding/PermissionAction");

const STALE = "onboarding.permissions.accessibility.stale";

test("trusted with a failed input probe is a stale entry", () => {
  expect(accessibilityStatus("granted", "failed")).toBe("stale");
});

test("trusted with a working input probe is granted", () => {
  expect(accessibilityStatus("granted", "ok")).toBe("granted");
});

test("an untrusted app is not granted, whatever the probe says", () => {
  expect(accessibilityStatus("needed", "not-run")).toBe("needed");
  expect(accessibilityStatus("needed", "failed")).toBe("needed");
  expect(accessibilityStatus("needed", "ok")).toBe("needed");
});

test("a check that couldn't complete stays an error, whatever the probe says", () => {
  expect(accessibilityStatus("error", "ok")).toBe("error");
  expect(accessibilityStatus("error", "failed")).toBe("error");
});

test("a trusted app whose probe didn't run is not granted", () => {
  expect(accessibilityStatus("granted", "not-run")).toBe("needed");
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

test("the permission card shows the stale steps for a stale entry", () => {
  const html = renderToStaticMarkup(
    createElement(PermissionAction, {
      status: "stale",
      grantLabel: "grant-label",
      onGrant: () => {},
      onRetry: () => {},
    }),
  );
  expect(html).toContain(`${STALE}.step1`);
  expect(html).toContain(`${STALE}.checkAgain`);
  expect(html).not.toContain("onboarding.permissions.granted");
});
