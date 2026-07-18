# Installing Coco Voice (macOS, Apple Silicon)

Coco Voice is distributed outside the App Store, so the first time you open it,
macOS Gatekeeper shows a warning. You only need to handle this **once per Mac** —
after the first install, Coco Voice updates itself silently in the background.

## Easiest: one-line install

Open **Terminal** and paste:

```
curl -fsSL https://raw.githubusercontent.com/coco-research/Coco-Voice/main/install.sh | bash
```

This downloads the latest release, installs it to `/Applications`, clears the
Gatekeeper flag, and launches Coco Voice.

## Or: double-click

Download **`Install Coco Voice.command`** from the release page and double-click it.
If macOS blocks it, right-click the file in Finder → **Open** → **Open**. It runs
the same one-line installer above.

## Or: fully manual

1. Open the DMG and drag **Coco Voice** into **Applications**.
2. In Terminal, run:
   ```
   xattr -dr com.apple.quarantine "/Applications/Coco Voice.app"
   ```
3. Launch Coco Voice from Applications.

## Why is this necessary?

The app is ad-hoc signed but not Apple-notarized. Full notarization requires a
paid Apple Developer account; until then, this one-time step is expected and
safe. It is not a sign that anything is wrong with the app.
