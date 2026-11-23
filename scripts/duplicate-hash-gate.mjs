#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function sha256File(filePath) {
  return crypto.createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

/**
 * Duplicate-hash gate for screenshot captures:
 *  - Fails (withinCheckpoint.length > 0) if any two shots in the same
 *    checkpoint and theme share the same sha256, returning the colliding pairs.
 *  - Reports any sha256 shared across different checkpoints (same theme).
 */
export function findDuplicateHashes(entries) {
  const byCkptTheme = new Map();
  const byTheme = new Map();
  const withinCheckpoint = [];
  const acrossCheckpoints = [];

  for (const entry of entries) {
    if (!entry || !entry.sha256) continue;
    const ckKey = `${entry.checkpoint}::${entry.theme}::${entry.sha256}`;
    const prevCk = byCkptTheme.get(ckKey);
    if (prevCk) {
      withinCheckpoint.push({
        checkpoint: entry.checkpoint,
        theme: entry.theme,
        sha256: entry.sha256,
        first: prevCk.id || prevCk.slug,
        second: entry.id || entry.slug,
      });
    } else {
      byCkptTheme.set(ckKey, entry);
    }

    const thKey = `${entry.theme}::${entry.sha256}`;
    const prevTh = byTheme.get(thKey);
    if (prevTh) {
      if (prevTh.checkpoint !== entry.checkpoint) {
        acrossCheckpoints.push({
          theme: entry.theme,
          sha256: entry.sha256,
          first: `${prevTh.checkpoint}/${prevTh.id || prevTh.slug}`,
          second: `${entry.checkpoint}/${entry.id || entry.slug}`,
        });
      }
    } else {
      byTheme.set(thKey, entry);
    }
  }

  return { withinCheckpoint, acrossCheckpoints };
}

export function runCheckDuplicatesCli(files) {
  const entries = files.map((f, idx) => ({
    id: path.basename(f),
    slug: path.basename(f),
    checkpoint: "CLI",
    theme: "single",
    sha256: sha256File(f),
    index: idx,
  }));
  const { withinCheckpoint } = findDuplicateHashes(entries);
  if (withinCheckpoint.length > 0) {
    for (const dup of withinCheckpoint) {
      console.error(
        `DUPLICATE_HASH_IN_CHECKPOINT: ${dup.checkpoint} (${dup.theme}) ${dup.first} == ${dup.second} sha256=${dup.sha256}`,
      );
    }
    process.exit(1);
  }
  console.log(`DUPLICATE_HASH_GATE_OK (${entries.length} distinct files)`);
  process.exit(0);
}

const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const checkDupIdx = process.argv.indexOf("--check-duplicates");
  const files =
    checkDupIdx >= 0
      ? process.argv.slice(checkDupIdx + 1).filter((a) => !a.startsWith("--"))
      : process.argv.slice(2).filter((a) => !a.startsWith("--"));
  runCheckDuplicatesCli(files);
}
