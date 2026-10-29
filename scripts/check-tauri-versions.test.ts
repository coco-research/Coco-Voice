import { expect, test } from "bun:test";
import { compareTauriVersions } from "./check-tauri-versions.ts";

function bunLock(packages: Record<string, string>): string {
  const entries = Object.entries(packages)
    .map(([name, version]) => `    "${name}": ["${name}@${version}",],`)
    .join("\n");
  return `{
  "packages": {
${entries}
  },
}
`;
}

function cargoLock(crates: Record<string, string[]>): string {
  return Object.entries(crates)
    .flatMap(([name, versions]) =>
      versions.map(
        (version) => `[[package]]\nname = "${name}"\nversion = "${version}"\n`,
      ),
    )
    .join("\n");
}

test("matching pair passes", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/plugin-os"],
    bunLock({ "@tauri-apps/plugin-os": "2.3.2" }),
    cargoLock({ "tauri-plugin-os": ["2.3.2"] }),
  );
  expect(report.mismatches).toEqual([]);
  expect(report.pairCount).toBe(1);
});

test("minor mismatch fails and names both sides", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/plugin-updater"],
    bunLock({ "@tauri-apps/plugin-updater": "2.10.0" }),
    cargoLock({ "tauri-plugin-updater": ["2.12.0"] }),
  );
  expect(report.mismatches).toEqual([
    {
      npmPackage: "@tauri-apps/plugin-updater",
      npmVersion: "2.10.0",
      crate: "tauri-plugin-updater",
      crateVersion: "2.12.0",
    },
  ]);
});

test("patch-only difference passes", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/plugin-os"],
    bunLock({ "@tauri-apps/plugin-os": "2.3.2" }),
    cargoLock({ "tauri-plugin-os": ["2.3.1"] }),
  );
  expect(report.mismatches).toEqual([]);
  expect(report.pairCount).toBe(1);
});

test("@tauri-apps/api is compared with the tauri crate", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/api"],
    bunLock({ "@tauri-apps/api": "2.10.1" }),
    cargoLock({ tauri: ["2.9.0"] }),
  );
  expect(report.mismatches).toEqual([
    {
      npmPackage: "@tauri-apps/api",
      npmVersion: "2.10.1",
      crate: "tauri",
      crateVersion: "2.9.0",
    },
  ]);
});

test("@tauri-apps/cli is ignored", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/cli"],
    bunLock({ "@tauri-apps/cli": "2.10.1" }),
    cargoLock({ tauri: ["9.9.9"] }),
  );
  expect(report.mismatches).toEqual([]);
  expect(report.pairCount).toBe(0);
});

test("npm package with no crate is skipped", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/plugin-sql", "@tauri-apps/plugin-os"],
    bunLock({
      "@tauri-apps/plugin-sql": "2.3.1",
      "@tauri-apps/plugin-os": "2.3.2",
    }),
    cargoLock({ "tauri-plugin-os": ["2.3.2"] }),
  );
  expect(report.mismatches).toEqual([]);
  expect(report.pairCount).toBe(1);
});

test("every Cargo.lock entry for a crate is compared", () => {
  const report = compareTauriVersions(
    ["@tauri-apps/plugin-os"],
    bunLock({ "@tauri-apps/plugin-os": "2.3.2" }),
    cargoLock({ "tauri-plugin-os": ["2.3.1", "2.4.0"] }),
  );
  expect(report.pairCount).toBe(2);
  expect(report.mismatches).toEqual([
    {
      npmPackage: "@tauri-apps/plugin-os",
      npmVersion: "2.3.2",
      crate: "tauri-plugin-os",
      crateVersion: "2.4.0",
    },
  ]);
});
