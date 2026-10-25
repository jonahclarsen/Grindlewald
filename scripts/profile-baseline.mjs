// Original lookup retained only for CI equivalence and CPU comparisons.
import { whiteAtPosition } from "../src/white-breathing.js";
function hexToRgb(value) {
  const hex = value.replace("#", "");
  return [0, 2, 4].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16));
}
export function whitePositionFromHex(value) {
  const target = hexToRgb(value);
  let bestPosition = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let step = 0; step <= 100; step += 1) {
    const candidate = hexToRgb(whiteAtPosition(step / 100));
    const distance = candidate.reduce((sum, channel, index) => sum + (channel - target[index]) ** 2, 0);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestPosition = step / 100;
    }
  }
  return bestPosition;
}
