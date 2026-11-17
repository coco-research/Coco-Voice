import { expect, mock, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

// Translation keys stand in for the strings, so the markup names what it shows.
mock.module("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { runPermissionCheck, windowsMicrophoneAccess } =
  await import("../src/lib/utils/permissions");
const { default: PermissionAction } =
  await import("../src/components/onboarding/PermissionAction");

const RETRY = "errors.shortcutRegistrationTryAgain";

function windowsStatus(
  overall_access: "allowed" | "denied" | "unknown",
  supported = true,
) {
  return {
    supported,
    overall_access,
    device_access: overall_access,
    app_access: overall_access,
    desktop_app_access: overall_access,
  };
}

test("a rejected permission check is not granted", async () => {
  expect(
    await runPermissionCheck(() => Promise.reject(new Error("plugin down"))),
  ).toBe("error");
});

test("a permission check that throws is not granted", async () => {
  expect(
    await runPermissionCheck(() => {
      throw new Error("no bridge");
    }),
  ).toBe("error");
});

test("an unknown permission state is not granted", async () => {
  expect(await runPermissionCheck(async () => null)).toBe("error");
  expect(
    await runPermissionCheck(async () =>
      windowsMicrophoneAccess(windowsStatus("unknown")),
    ),
  ).toBe("error");
});

test("only a definite answer decides granted or needed", async () => {
  expect(await runPermissionCheck(async () => true)).toBe("granted");
  expect(await runPermissionCheck(async () => false)).toBe("needed");
  expect(windowsMicrophoneAccess(windowsStatus("allowed"))).toBe(true);
  expect(windowsMicrophoneAccess(windowsStatus("denied"))).toBe(false);
  expect(windowsMicrophoneAccess(windowsStatus("unknown", false))).toBe(true);
});

function renderAction(status: "error" | "needed" | "granted"): string {
  return renderToStaticMarkup(
    createElement(PermissionAction, {
      status,
      grantLabel: "grant-label",
      onGrant: () => {},
      onRetry: () => {},
    }),
  );
}

test("onboarding offers Retry when a check couldn't complete", () => {
  const html = renderAction("error");
  expect(html).toContain(RETRY);
  expect(html).toContain("onboarding.permissions.errors.checkFailed");
  expect(html).not.toContain("onboarding.permissions.granted");
});

test("onboarding shows no Retry for a plain not-granted or granted state", () => {
  expect(renderAction("needed")).not.toContain(RETRY);
  expect(renderAction("needed")).toContain("grant-label");
  expect(renderAction("granted")).not.toContain(RETRY);
  expect(renderAction("granted")).toContain("onboarding.permissions.granted");
});
