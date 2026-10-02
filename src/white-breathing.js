const anchors = [
  [2000, [255, 141, 11]], [2700, [255, 169, 87]],
  [5500, [255, 238, 222]], [7500, [238, 239, 255]], [9000, [217, 225, 255]],
];

export function kelvinAtPosition(position) {
  return Math.round(2000 + Math.max(0, Math.min(1, position)) * 7000);
}

export function whiteAtPosition(position) {
  const kelvin = kelvinAtPosition(position);
  const upperIndex = anchors.findIndex(([temperature]) => temperature >= kelvin);
  const [lowKelvin, lowRgb] = anchors[Math.max(0, upperIndex - 1)];
  const [highKelvin, highRgb] = anchors[Math.max(0, upperIndex)];
  const amount = highKelvin === lowKelvin ? 0 : (kelvin - lowKelvin) / (highKelvin - lowKelvin);
  return "#" + lowRgb.map((channel, index) => Math.round(channel + (highRgb[index] - channel) * amount).toString(16).padStart(2, "0")).join("");
}

export function whitePlaybackPosition(phase) {
  const normalized = ((phase % 1530) + 1530) % 1530;
  return Math.min(normalized, 1530 - normalized) / 765;
}

export function whiteSeekPhase(position, previousPhase = 0) {
  const forward = Math.round(Math.max(0, Math.min(1, position)) * 765);
  return (previousPhase > 765 ? 1530 - forward : forward) % 1530;
}

export function whiteBreathingCommand(sweepSeconds) {
  if (!Number.isInteger(sweepSeconds) || sweepSeconds < 5 || sweepSeconds > 120 || sweepSeconds % 5 !== 0) {
    throw new Error("Choose a sweep time from 5 to 120 seconds, in steps of 5.");
  }
  return { command: "breathe_white", sweep_seconds: sweepSeconds, device: null };
}

export function whiteBreathingPaceCommand(sweepSeconds) {
  const { sweep_seconds } = whiteBreathingCommand(sweepSeconds);
  return { command: "set_white_breathing_pace", sweep_seconds };
}
