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
  const keyboardImplementation: string | undefined = getSetting(
    "keyboard_implementation",
  );

  // Default to Tauri implementation if not set. Accept "coco_voice_keys" or legacy "coco_keys"
  if (
    keyboardImplementation === "coco_voice_keys" ||
    keyboardImplementation === "coco_keys"
  ) {
    return <CocoVoiceKeysShortcutInput {...props} />;
  }

  return <GlobalShortcutInput {...props} />;
};
