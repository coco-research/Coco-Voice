import React from "react";
import {
  AgentAskPillView,
  HandsFreePillView,
  MeetingPillView,
  RecordingPillShowcase,
  RecordingPillStateView,
} from "../src/overlay/RecordingPillStates";

export type PillHarnessScene =
  | "all"
  | "listening"
  | "streaming"
  | "working"
  | "sent"
  | "sent-unknown"
  | "clipboard"
  | "kept"
  | "hands-free"
  | "meeting"
  | "meeting-waiting"
  | "agent-ask"
  | "agent-answered"
  | "agent-queued";

export const PillStatesHarness: React.FC<{ scene: PillHarnessScene }> = ({
  scene,
}) => {
  switch (scene) {
    case "listening":
      return <RecordingPillStateView kind="listening" />;
    case "streaming":
      return (
        <RecordingPillStateView
          kind="streaming"
          streamingWords="can we move the review to…"
        />
      );
    case "working":
      return <RecordingPillStateView kind="working" />;
    case "sent":
      return <RecordingPillStateView kind="sent" targetApp="Mail" />;
    case "sent-unknown":
      return <RecordingPillStateView kind="sent" />;
    case "clipboard":
      return <RecordingPillStateView kind="clipboard" />;
    case "kept":
      return <RecordingPillStateView kind="kept_press_paste" isMac={true} />;
    case "hands-free":
      return <HandsFreePillView shortcutKey="fn" />;
    case "meeting":
      return <MeetingPillView elapsedSecs={768} onStop={() => {}} />;
    case "meeting-waiting":
      return (
        <MeetingPillView
          elapsedSecs={768}
          waitingAgentQuestions={2}
          onStop={() => {}}
        />
      );
    case "agent-ask":
      return (
        <AgentAskPillView
          agentName="Claude Code"
          question="Which branch?"
          shortcutKey="fn"
        />
      );
    case "agent-answered":
      return (
        <AgentAskPillView
          agentName="Claude Code"
          question=""
          answerSentTo="Claude Code"
        />
      );
    case "agent-queued":
      return (
        <AgentAskPillView
          agentName="Claude Code"
          question="Which branch?"
          shortcutKey="fn"
          queuedCount={1}
        />
      );
    case "all":
    default:
      return <RecordingPillShowcase />;
  }
};
