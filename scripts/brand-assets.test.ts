import { expect, test } from "bun:test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { inflateSync } from "node:zlib";

const root = join(import.meta.dir, "..");

function pngDimensions(path: string): { width: number; height: number } {
  const buf = readFileSync(path);
  expect(
    buf.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])),
  ).toBe(true);
  return {
    width: buf.readUInt32BE(16),
    height: buf.readUInt32BE(20),
  };
}

function unfilterPngRgba(path: string): {
  width: number;
  height: number;
  rgba: Uint8Array;
} {
  const buf = readFileSync(path);
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  const bitDepth = buf[24];
  const colorType = buf[25];
  expect(bitDepth).toBe(8);
  const channels = colorType === 6 ? 4 : colorType === 2 ? 3 : 0;
  expect(channels).toBeGreaterThan(0);

  const idatParts: Buffer[] = [];
  let pos = 8;
  while (pos + 8 <= buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.subarray(pos + 4, pos + 8).toString("ascii");
    if (type === "IDAT") {
      idatParts.push(buf.subarray(pos + 8, pos + 8 + len));
    }
    if (type === "IEND") break;
    pos += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idatParts));
  const stride = width * channels;
  const recon = new Uint8Array(height * stride);
  const paeth = (a: number, b: number, c: number) => {
    const p = a + b - c;
    const pa = Math.abs(p - a);
    const pb = Math.abs(p - b);
    const pc = Math.abs(p - c);
    if (pa <= pb && pa <= pc) return a;
    return pb <= pc ? b : c;
  };

  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const rowIn = y * (stride + 1) + 1;
    const rowOut = y * stride;
    for (let i = 0; i < stride; i++) {
      const x = raw[rowIn + i];
      const a = i >= channels ? recon[rowOut + i - channels] : 0;
      const b = y > 0 ? recon[rowOut - stride + i] : 0;
      const c =
        y > 0 && i >= channels ? recon[rowOut - stride + i - channels] : 0;
      let val = x;
      if (filter === 1) val = (x + a) & 0xff;
      else if (filter === 2) val = (x + b) & 0xff;
      else if (filter === 3) val = (x + ((a + b) >> 1)) & 0xff;
      else if (filter === 4) val = (x + paeth(a, b, c)) & 0xff;
      recon[rowOut + i] = val;
    }
  }

  const rgba = new Uint8Array(width * height * 4);
  for (let p = 0; p < width * height; p++) {
    rgba[p * 4] = recon[p * channels];
    rgba[p * 4 + 1] = recon[p * channels + 1];
    rgba[p * 4 + 2] = recon[p * channels + 2];
    rgba[p * 4 + 3] = channels === 4 ? recon[p * 4 + 3] : 255;
  }
  return { width, height, rgba };
}

test("Coco Research pure-orb app and web icons exist with expected dimensions and zero <=32px wordmark", () => {
  const expectedSizes: Array<[string, number, number]> = [
    ["src-tauri/icons/32x32.png", 32, 32],
    ["src-tauri/icons/64x64.png", 64, 64],
    ["src-tauri/icons/128x128.png", 128, 128],
    ["src-tauri/icons/128x128@2x.png", 256, 256],
    ["src-tauri/icons/Square30x30Logo.png", 30, 30],
    ["src-tauri/icons/StoreLogo.png", 50, 50],
    ["src-tauri/icons/icon.png", 512, 512],
    ["src-tauri/icons/logo.png", 512, 512],
    ["src-tauri/resources/coco-voice.png", 64, 64],
    ["src-tauri/resources/tray_idle.png", 64, 64],
    ["src-tauri/resources/tray_idle_dark.png", 64, 64],
    ["public/favicon-16.png", 16, 16],
    ["public/favicon-32.png", 32, 32],
    ["public/logo.png", 512, 512],
  ];

  for (const [rel, w, h] of expectedSizes) {
    const full = join(root, rel);
    expect(existsSync(full)).toBe(true);
    expect(pngDimensions(full)).toEqual({ width: w, height: h });
  }

  expect(existsSync(join(root, "src-tauri/icons/icon.icns"))).toBe(true);
  expect(existsSync(join(root, "src-tauri/icons/icon.ico"))).toBe(true);
  expect(existsSync(join(root, "assets/branding/coco-research-orb.svg"))).toBe(
    true,
  );

  const smallIcons = [
    "public/favicon-16.png",
    "public/favicon-32.png",
    "src-tauri/icons/32x32.png",
    "src-tauri/icons/Square30x30Logo.png",
    "src-tauri/icons/ios/AppIcon-20x20@1x.png",
    "src-tauri/icons/ios/AppIcon-29x29@1x.png",
  ];
  for (const rel of smallIcons) {
    const { width, height, rgba } = unfilterPngRgba(join(root, rel));
    const yStart = Math.max(0, height - Math.max(3, Math.ceil(height * 0.16)));
    let brightBottomPixels = 0;
    for (let y = yStart; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const idx = (y * width + x) * 4;
        const luma =
          (0.2126 * rgba[idx] +
            0.7152 * rgba[idx + 1] +
            0.0722 * rgba[idx + 2]) *
          (rgba[idx + 3] / 255);
        if (luma > 115) brightBottomPixels++;
      }
    }
    expect(brightBottomPixels).toBe(0);
  }

  const brandingFiles = readdirSync(join(root, "assets/branding"));
  expect(brandingFiles.some((f) => f.includes("parrot"))).toBe(false);
  expect(
    readFileSync(join(root, "REBRAND.md"), "utf8")
      .toLowerCase()
      .includes("parrot"),
  ).toBe(false);
});

test("macOS DMG and Windows NSIS installer art match Tauri bundle config and BMP specs", () => {
  const tauriConf = JSON.parse(
    readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const dmgBgRel = tauriConf.bundle?.macOS?.dmg?.background;
  const nsisHeaderRel = tauriConf.bundle?.windows?.nsis?.headerImage;
  const nsisSidebarRel = tauriConf.bundle?.windows?.nsis?.sidebarImage;

  expect(dmgBgRel).toBe("../assets/branding/dmg-background.png");
  expect(nsisHeaderRel).toBe("../assets/branding/nsis-header.bmp");
  expect(nsisSidebarRel).toBe("../assets/branding/nsis-sidebar.bmp");

  const dmg1x = join(root, "src-tauri", dmgBgRel);
  const dmg2x = join(root, "assets/branding/dmg-background@2x.png");
  expect(pngDimensions(dmg1x)).toEqual({ width: 660, height: 400 });
  expect(pngDimensions(dmg2x)).toEqual({ width: 1320, height: 800 });

  const checkBmp = (p: string, w: number, h: number) => {
    const buf = readFileSync(p);
    expect(buf.subarray(0, 2).toString("ascii")).toBe("BM");
    expect(buf.readInt32LE(18)).toBe(w);
    expect(Math.abs(buf.readInt32LE(22))).toBe(h);
    expect(buf.readUInt16LE(28)).toBe(24);
    expect(buf.readUInt32LE(30)).toBe(0);
  };
  checkBmp(join(root, "src-tauri", nsisHeaderRel), 150, 57);
  checkBmp(join(root, "src-tauri", nsisSidebarRel), 164, 314);
});

test("README light/dark picture banner, 1280x640 social preview, and cross-OS INSTALL guide match brand spec", () => {
  expect(pngDimensions(join(root, "assets/branding/banner-dark.png"))).toEqual({
    width: 1500,
    height: 500,
  });
  expect(pngDimensions(join(root, "assets/branding/banner-light.png"))).toEqual(
    {
      width: 1500,
      height: 500,
    },
  );
  expect(
    pngDimensions(join(root, "assets/branding/social-preview-1280x640.png")),
  ).toEqual({
    width: 1280,
    height: 640,
  });

  const readme = readFileSync(join(root, "README.md"), "utf8");
  expect(readme.includes("<picture>")).toBe(true);
  expect(readme.includes("assets/branding/banner-dark.png")).toBe(true);
  expect(readme.includes("assets/branding/banner-light.png")).toBe(true);
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
    version: string;
  };
  const majorMinor = pkg.version.split(".").slice(0, 2).join(".");
  expect(readme.includes(majorMinor)).toBe(true);
  expect(readme.toLowerCase().includes("parrot")).toBe(false);

  const install = readFileSync(join(root, "INSTALL.md"), "utf8");
  expect(install.includes("Get-FileHash")).toBe(true);
  expect(install.includes("SmartScreen")).toBe(true);
  expect(install.includes("AppImage")).toBe(true);
  expect(install.includes("SHA256SUMS")).toBe(true);
});
