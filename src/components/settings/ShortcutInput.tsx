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
 * - "tauri" (default): Uses GlobalShortcutInput with JS keyboard events
 * - "coco_keys": Uses CocoVoiceKeysShortcutInput with backend key events
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = (props) => {
  const { getSetting } = useSettings();
  const keyboardImplementation = getSetting("keyboard_implementation");

  // Default to Tauri implementation if not set. Accept "coco_keys" or legacy "handy_keys"
  if (keyboardImplementation === "coco_keys" || keyboardImplementation === "handy_keys") {
    return <CocoVoiceKeysShortcutInput {...props} />;
  }

  return <GlobalShortcutInput {...props} />;
};
