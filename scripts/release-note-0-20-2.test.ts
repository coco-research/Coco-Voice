import { describe, expect, test } from "bun:test";
import { statSync } from "node:fs";
import { join } from "node:path";
import { createServer } from "vite";

const root = join(import.meta.dir, "..");

async function loadReleaseNotesModule() {
  const server = await createServer({
    root,
    configFile: false,
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
    logLevel: "silent",
    optimizeDeps: { noDiscovery: true },
  });
  try {
    const mod = (await server.ssrLoadModule(
      "/src/components/whats-new/releaseNotes.ts",
    )) as typeof import("../src/components/whats-new/releaseNotes");
    return mod;
  } finally {
    await server.close();
  }
}

describe("0.20.2 empty release note", () => {
  test("findReleaseNoteToShow returns 0.20.2 with empty markdown for users who last saw 0.10.1", async () => {
    const { findReleaseNoteToShow } = await loadReleaseNotesModule();
    const note = findReleaseNoteToShow({
      currentVersion: "0.20.3",
      lastSeenVersion: "0.10.1",
    });

    expect(note).not.toBeNull();
    expect(note?.version).toBe("0.20.2");
    expect(note?.markdown.trim()).toBe("");
    expect(
      statSync(join(root, "src/content/release-notes/0.20.2.md")).size,
    ).toBe(0);
  });
});
