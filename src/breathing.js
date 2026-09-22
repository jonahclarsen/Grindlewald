export function minimumCycleSeconds(colorStep) {
  return Math.ceil(Math.ceil(1530 / colorStep) * 300 / 1000);
}

export function formatCycleSeconds(seconds) {
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return minutes ? `${minutes}m ${remainder}s` : `${remainder}s`;
}

export function cycleSecondsAfterStepChange(seconds, previousStep, nextStep) {
  const previousMinimum = minimumCycleSeconds(previousStep);
  const nextMinimum = minimumCycleSeconds(nextStep);
  return seconds <= previousMinimum ? nextMinimum : Math.max(seconds, nextMinimum);
}
