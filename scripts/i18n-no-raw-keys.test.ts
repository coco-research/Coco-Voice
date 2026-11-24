import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { findMissingStaticI18nKeys } from "./i18n-key-scan";

const rootDir = join(import.meta.dir, "..");

describe("Static i18n keys", () => {
  test("every static t() / i18nKey call without a default value exists in en", () => {
    const missing = findMissingStaticI18nKeys(rootDir, {
      onlyWithoutDefault: true,
    });
    expect(missing).toEqual([]);
  });

  test("every static t() / i18nKey call (with or without a default) exists in en", () => {
    const missing = findMissingStaticI18nKeys(rootDir, {
      onlyWithoutDefault: false,
    });
    expect(missing).toEqual([]);
  });
});
