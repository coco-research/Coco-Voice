import { useEffect, useState, useRef, type ReactNode } from "react";
import { toast, Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import {
  ModelStateEvent,
  RecordingErrorEvent,
  ShortcutRegistrationFailure,
} from "./lib/types/events";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import Onboarding, { AccessibilityOnboarding } from "./components/onboarding";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useMeetingsUi } from "./components/meetings/gate";
import { WhatsNewGate } from "./components/whats-new";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import { commands, events } from "@/bindings";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type OnboardingStep = "accessibility" | "model" | "done";

const renderSettingsContent = (section: keyof typeof SECTIONS_CONFIG) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function shortcutFailureBodyKey(
  cause: ShortcutRegistrationFailure["cause"],
): string {
  switch (cause) {
    case "taken":
      return "errors.shortcutRegistrationFailedTaken";
    case "rejected":
      return "errors.shortcutRegistrationFailedRejected";
    case "other":
    default:
      return "errors.shortcutRegistrationFailedOther";
  }
}

function App() {
  const { t, i18n } = useTranslation();
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep | null>(
    null,
  );
  // Track if this is a returning user who just needs to grant permissions
  // (vs a new user who needs full onboarding including model selection)
  const [isReturningUser, setIsReturningUser] = useState(false);
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("general");
  const { settings, updateSetting } = useSettings();
  const meetingsUi = useMeetingsUi();
  const showMeetings =
    meetingsUi !== null && meetingsUi.sidebarVisible(settings);
  const direction = getLanguageDirection(i18n.language);
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const hasCompletedPostOnboardingInit = useRef(false);
  const clipboardNoticeShown = useRef(false);

  useEffect(() => {
    checkOnboardingStatus();
  }, []);

  useEffect(() => {
    if (currentSection === "meetings" && !showMeetings) {
      setCurrentSection("general");
    }
  }, [currentSection, showMeetings]);

  // Initialize RTL direction when language changes
  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  // A failed registration stays uninitialized on the Rust side, so a later
  // initializeShortcuts() tries again. This listener has to be up first.
  useEffect(() => {
    const unlisten = listen<ShortcutRegistrationFailure[]>(
      "shortcut-registration-failed",
      (event) => {
        const failures = event.payload;
        const first = failures[0];
        if (!first) return;
        const more =
          failures.length > 1
            ? t("errors.shortcutRegistrationFailedMore", {
                count: failures.length - 1,
              })
            : "";
        // A dead shortcut leaves the app unusable, and a tray app's window is
        // often hidden: bring it forward and keep the notice until dismissed.
        void commands.showMainWindowCommand();
        toast(t("errors.shortcutRegistrationFailedTitle"), {
          description:
            t(shortcutFailureBodyKey(first.cause), {
              binding: first.binding,
            }) + more,
          duration: Infinity,
          action: {
            label: t("errors.shortcutRegistrationOpenSettings"),
            onClick: () => setCurrentSection("general"),
          },
          cancel: {
            label: t("errors.shortcutRegistrationTryAgain"),
            onClick: () => {
              void commands.initializeShortcuts();
            },
          },
        });
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t, setCurrentSection]);

  // Initialize Enigo, shortcuts, and refresh audio devices when main app loads
  useEffect(() => {
    if (onboardingStep === "done" && !hasCompletedPostOnboardingInit.current) {
      hasCompletedPostOnboardingInit.current = true;
      Promise.all([
        commands.initializeEnigo(),
        commands.initializeShortcuts(),
      ]).catch((e) => {
        console.warn("Failed to initialize:", e);
      });
      refreshAudioDevices();
      refreshOutputDevices();
    }
  }, [onboardingStep, refreshAudioDevices, refreshOutputDevices]);

  // Handle keyboard shortcuts for debug mode toggle
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Check for Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
      const isDebugShortcut =
        event.shiftKey &&
        event.key.toLowerCase() === "d" &&
        (event.ctrlKey || event.metaKey);

      if (isDebugShortcut) {
        event.preventDefault();
        const currentDebugMode = settings?.debug_mode ?? false;
        updateSetting("debug_mode", !currentDebugMode);
      }
    };

    // Add event listener when component mounts
    document.addEventListener("keydown", handleKeyDown);

    // Cleanup event listener when component unmounts
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

  // Listen for recording errors from the backend and show a toast
  useEffect(() => {
    const unlisten = listen<RecordingErrorEvent>("recording-error", (event) => {
      const { error_type, detail } = event.payload;

      if (error_type === "microphone_permission_denied") {
        const currentPlatform = platform();
        const platformKey = `errors.micPermissionDenied.${currentPlatform}`;
        const description = t(platformKey, {
          defaultValue: t("errors.micPermissionDenied.generic"),
        });
        toast.error(t("errors.micPermissionDeniedTitle"), { description });
      } else if (error_type === "no_input_device") {
        toast.error(t("errors.noInputDeviceTitle"), {
          description: t("errors.noInputDevice"),
        });
      } else {
        toast.error(
          t("errors.recordingFailed", { error: detail ?? "Unknown error" }),
        );
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Rust picks the variant from where the words actually are. The technical
  // error, when there is one, stays in coco-voice.log.
  useEffect(() => {
    const pasteNoticeKeys: Record<string, string> = {
      clipboard_and_history: "errors.pasteNotice.clipboardAndHistory",
      clipboard_only: "errors.pasteNotice.clipboardOnly",
      history_only: "errors.pasteNotice.historyOnly",
      neither: "errors.pasteNotice.neither",
    };
    const unlisten = listen<string>("paste-notice", (event) => {
      const key = pasteNoticeKeys[event.payload];
      if (key) toast(t(key));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  useEffect(() => {
    if (onboardingStep === null) return;
    if (
      !settings?.clipboard_default_notice_pending ||
      clipboardNoticeShown.current
    ) {
      return;
    }
    clipboardNoticeShown.current = true;
    // Clear the flag only when the user closes the notice. The app can start
    // hidden in the tray, so showing it is not the same as the user seeing it.
    const dismiss = () => {
      commands
        .dismissClipboardDefaultNotice()
        .then((result) => {
          if (result.status === "ok") {
            useSettingsStore.setState((state) => ({
              settings: state.settings
                ? { ...state.settings, clipboard_default_notice_pending: false }
                : state.settings,
            }));
          }
        })
        .catch((error) => {
          console.warn("Failed to dismiss clipboard notice:", error);
        });
    };
    toast(t("notices.clipboardDefault"), {
      duration: Infinity,
      closeButton: true,
      onDismiss: dismiss,
      classNames: {
        toast:
          "relative max-w-sm items-start bg-background border border-mid-gray/20 rounded-lg shadow-lg px-4 py-3 pe-8 flex gap-3 text-sm text-left",
        closeButton:
          "absolute top-2 end-2 flex h-5 w-5 items-center justify-center rounded-full border border-mid-gray/30 bg-background text-text cursor-pointer",
      },
    });
  }, [onboardingStep, settings?.clipboard_default_notice_pending, t]);

  // Listen for transcription failures and show a toast.
  // The payload is the backend error message (also logged to coco-voice.log).
  useEffect(() => {
    const unlisten = listen<string>("transcription-error", (event) => {
      toast.error(t("errors.transcriptionFailedTitle"), {
        description: event.payload,
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Listen for model loading failures and show a toast
  useEffect(() => {
    const unlisten = listen<ModelStateEvent>("model-state-changed", (event) => {
      if (event.payload.event_type === "loading_failed") {
        toast.error(
          t("errors.modelLoadFailed", {
            model:
              event.payload.model_name || t("errors.modelLoadFailedUnknown"),
          }),
          {
            description: event.payload.error,
          },
        );
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // The banner disappears after the next take where cleanup runs.
  // We know a take happened when historyUpdatePayload is emitted.
  useEffect(() => {
    let statusSeenThisTake = false;

    const unlistenCleanup = events.cleanupStatus.listen((event) => {
      if (event.payload.needs_action) {
        useSettingsStore.getState().setLastCleanupStatus(event.payload);
        statusSeenThisTake = true;
      }
    });

    const unlistenHistory = events.historyUpdatePayload.listen((event) => {
      // Only a new entry is a finished take; edits and deletes are not.
      if (event.payload.action !== "added") return;
      if (!statusSeenThisTake) {
        useSettingsStore.getState().setLastCleanupStatus(null);
      }
      statusSeenThisTake = false;
    });

    return () => {
      unlistenCleanup.then((fn) => fn());
      unlistenHistory.then((fn) => fn());
    };
  }, []);

  const revealMainWindowForPermissions = async () => {
    try {
      await commands.showMainWindowCommand();
    } catch (e) {
      console.warn("Failed to show main window for permission onboarding:", e);
    }
  };

  const checkOnboardingStatus = async () => {
    try {
      const settingsResult = await commands.getAppSettings();
      const hasCompletedOnboarding =
        settingsResult.status === "ok" &&
        settingsResult.data.onboarding_completed === true;
      const currentPlatform = platform();

      if (hasCompletedOnboarding) {
        // Returning user - check if they need to grant permissions first
        setIsReturningUser(true);

        if (currentPlatform === "macos") {
          try {
            const [hasAccessibility, hasMicrophone] = await Promise.all([
              checkAccessibilityPermission(),
              checkMicrophonePermission(),
            ]);
            if (!hasAccessibility || !hasMicrophone) {
              await revealMainWindowForPermissions();
              setOnboardingStep("accessibility");
              return;
            }
          } catch (e) {
            console.warn("Failed to check macOS permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        if (currentPlatform === "windows") {
          try {
            const microphoneStatus =
              await commands.getWindowsMicrophonePermissionStatus();
            if (
              microphoneStatus.supported &&
              microphoneStatus.overall_access === "denied"
            ) {
              await revealMainWindowForPermissions();
              setOnboardingStep("accessibility");
              return;
            }
          } catch (e) {
            console.warn("Failed to check Windows microphone permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        setOnboardingStep("done");
      } else {
        // New user - start full onboarding
        setIsReturningUser(false);
        setOnboardingStep("accessibility");
      }
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setOnboardingStep("accessibility");
    }
  };

  const handleAccessibilityComplete = () => {
    // Returning users already have models, skip to main app
    // New users need to select a model
    setOnboardingStep(isReturningUser ? "done" : "model");
  };

  const handleModelSelected = () => {
    // Transition to main app - user has started a download
    setOnboardingStep("done");
  };

  // Rendered once around every step below (including onboarding) so
  // toast.error() calls surface to the user. sonner renders via a portal, so
  // its position in the tree doesn't affect layout. Without this, errors during
  // onboarding (e.g. a model download failing because blob.handy.computer is
  // unreachable) are silently swallowed and the wizard just appears to "blink".
  const toaster = (
    <Toaster
      theme="system"
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            "bg-background border border-mid-gray/20 rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 text-sm",
          title: "font-medium",
          description: "text-mid-gray",
          actionButton:
            "cursor-pointer shrink-0 rounded-md border border-mid-gray/20 px-2 py-1 text-xs font-medium",
        },
      }}
    />
  );

  // Still checking onboarding status
  if (onboardingStep === null) {
    return null;
  }

  // Select the content for the current step. The Toaster is rendered once, in a
  // stable wrapper around this node, so crossing between onboarding steps and
  // the main app never remounts it (which would drop any in-flight toast).
  let content: ReactNode;
  if (onboardingStep === "accessibility") {
    content = (
      <AccessibilityOnboarding onComplete={handleAccessibilityComplete} />
    );
  } else if (onboardingStep === "model") {
    content = <Onboarding onModelSelected={handleModelSelected} />;
  } else {
    content = (
      <div
        dir={direction}
        className="h-screen flex flex-col select-none cursor-default"
      >
        <WhatsNewGate />
        {/* Main content area that takes remaining space */}
        <div className="flex-1 flex overflow-hidden">
          <Sidebar
            activeSection={currentSection}
            onSectionChange={setCurrentSection}
          />
          {/* Scrollable content area */}
          <div className="flex-1 flex flex-col overflow-hidden">
            <div className="flex-1 overflow-y-auto">
              <div className="flex flex-col items-center p-4 gap-4">
                <AccessibilityPermissions />
                {currentSection === "meetings" && meetingsUi ? (
                  <meetingsUi.MeetingsView />
                ) : (
                  renderSettingsContent(
                    currentSection === "meetings" ? "general" : currentSection,
                  )
                )}
              </div>
            </div>
          </div>
        </div>
        {/* Fixed footer at bottom */}
        <Footer />
      </div>
    );
  }

  return (
    <>
      {toaster}
      {content}
    </>
  );
}

export default App;
