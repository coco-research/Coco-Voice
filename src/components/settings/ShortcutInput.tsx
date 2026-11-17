import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { GlobalShortcutInput } from "./GlobalShortcutInput";
import { CocoVoiceKeysShortcutInput } from "./CocoVoiceKeysShortcutInput";

interface ShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

/**
 * Wrapper component that selects the appropriate shortcut input implementation
 * based on the keyboard_implementation setting.
 *
 * - "coco_voice_keys" (default on macOS and Windows): Uses CocoVoiceKeysShortcutInput with backend key events
 * - "tauri" (default on Linux, or not set): Uses GlobalShortcutInput with JS keyboard events
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = (props) => {
  const { getSetting } = useSettings();
  const keyboardImplementation = getSetting("keyboard_implementation");

  if (keyboardImplementation === "coco_voice_keys") {
    return <CocoVoiceKeysShortcutInput {...props} />;
  }

  return <GlobalShortcutInput {...props} />;
};
