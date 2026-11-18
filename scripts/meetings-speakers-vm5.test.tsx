import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

test("AboutSettings lists WeSpeaker CC-BY-4.0 and pyannote-segmentation-3.0 MIT credits", () => {
  const about = readFileSync(
    join(root, "src/components/settings/about/AboutSettings.tsx"),
    "utf8",
  );
  expect(about).toContain("WeSpeaker (wespeaker_en_voxceleb_resnet34.onnx)");
  expect(about).toContain("CC-BY-4.0");
  expect(about).toContain("https://github.com/wenet-e2e/wespeaker");
  expect(about).toContain("pyannote-segmentation-3.0");
  expect(about).toContain("MIT");
  expect(about).toContain("https://huggingface.co/pyannote/segmentation-3.0");
});

test("MeetingsView matches Frame 8 speaker rename copy and structure", () => {
  const ui = readFileSync(
    join(root, "src/components/meetings/meetings-ui.tsx"),
    "utf8",
  );
  expect(ui).toContain("Names in this meeting only.");
  expect(ui).toContain("No name typed for this meeting.");
  expect(ui).toContain("Renamed in this meeting.");
  expect(ui).toContain("Name for");
  expect(ui).toContain(
    "Speaker labels are worked out per meeting and nothing about anyone's voice is saved.",
  );
});
