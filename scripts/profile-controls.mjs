// Synthetic CPU profile. Runs only in CI; does not connect to lights.
import { readFileSync, writeFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { whiteAtPosition } from '../src/white-breathing.js';
const source = readFileSync('src/app.js', 'utf8');
const extract = (name) => source.match(new RegExp(`function ${name}\\([^]*?\\n}`))[0];
const legacy = new Function('whiteAtPosition', `${extract('hexToRgb')}\n${extract('whitePositionFromHex')}\nreturn whitePositionFromHex;`)(whiteAtPosition);
const optimized = (await import('../src/white-breathing.js')).whitePositionFromHex ?? legacy;
const colors = Array.from({length: 4096}, (_, i) => '#' + ((i * 4093) % 0x1000000).toString(16).padStart(6, '0'));
for (const color of colors) {
  if (legacy(color) !== optimized(color)) throw new Error(`Position changed for synthetic color ${color}`);
}
function measure(fn) {
  let checksum = 0;
  const start = performance.now();
  for (let pass = 0; pass < 4; pass++) for (const color of colors) checksum += fn(color);
  return {ms: performance.now() - start, checksum};
}
measure(legacy); measure(optimized);
const baseline = [], candidate = [];
for (let i = 0; i < 5; i++) {
  baseline.push(measure(legacy).ms); candidate.push(measure(optimized).ms);
}
const median = values => values.toSorted((a,b) => a-b)[2];
const report = {baselineMs: median(baseline), candidateMs: median(candidate), speedup: median(baseline)/median(candidate), colors: colors.length, identical: true};
writeFileSync('control-profile.json', JSON.stringify(report, null, 2));
console.log(report);
