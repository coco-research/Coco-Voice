export type OnboardingStep = "accessibility" | "download" | "try" | "done";

export function resolveInitialOnboardingStep(params: {
  hasModels: boolean;
  onboardingCompleted: boolean;
  missingPermission: boolean;
}): { step: OnboardingStep; isReturningUser: boolean } {
  if (params.hasModels || params.onboardingCompleted) {
    return {
      step: params.missingPermission ? "accessibility" : "done",
      isReturningUser: true,
    };
  }
  return { step: "accessibility", isReturningUser: false };
}
