import type { MeetingOffer, MeetingState } from "../../bindings.meetings";

export interface OfferViewState {
  offer: MeetingOffer | null;
  held: boolean;
}

export const INITIAL_OFFER_VIEW_STATE: OfferViewState = {
  offer: null,
  held: false,
};

/**
 * Approved Frame 1 surfaces the floating offer only when a call is active
 * (`inCall: true` for Zoom and Teams) or when a Google Meet call tab is open
 * (`app === "meet"`). Idle desktop windows ("Zoom is open." / "Teams is open.")
 * stay silent.
 */
export function shouldShowOffer(
  offer: MeetingOffer | null,
): offer is MeetingOffer {
  return offer !== null && (offer.inCall || offer.app === "meet");
}

export function sameOffer(a: MeetingOffer, b: MeetingOffer): boolean {
  return a.bundleId === b.bundleId && a.app === b.app && a.inCall === b.inCall;
}

export function offerBodyKey(
  offer: MeetingOffer,
):
  | "meetings.offer.meet"
  | "meetings.offer.teamsInCall"
  | "meetings.offer.zoomInCall" {
  if (offer.app === "meet") return "meetings.offer.meet";
  if (offer.app === "teams") return "meetings.offer.teamsInCall";
  return "meetings.offer.zoomInCall";
}

export function applyOfferEvent(
  state: OfferViewState,
  next: MeetingOffer | null,
): OfferViewState {
  if (!shouldShowOffer(next)) {
    return INITIAL_OFFER_VIEW_STATE;
  }
  if (state.offer && sameOffer(state.offer, next)) {
    return state;
  }
  return {
    offer: next,
    held: false,
  };
}

export function markOfferHeld(state: OfferViewState): OfferViewState {
  if (!state.offer) return INITIAL_OFFER_VIEW_STATE;
  return {
    ...state,
    held: true,
  };
}

export function applyMeetingStatus(
  state: OfferViewState,
  meetingState: MeetingState,
): OfferViewState {
  if (meetingState === "idle") {
    return {
      ...state,
      held: false,
    };
  }
  if (meetingState === "consent_open") {
    return state.offer ? { ...state, held: true } : state;
  }
  return INITIAL_OFFER_VIEW_STATE;
}

export function isOfferVisible(
  state: OfferViewState,
  meetingsAllowed: boolean,
): boolean {
  return meetingsAllowed && state.offer !== null && !state.held;
}
