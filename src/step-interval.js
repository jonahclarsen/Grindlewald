// The H6001 (Classic) is assumed to keep frames as closely spaced as the H6005
// until it is measured, so enabled H6001 lights show a reminder to measure them.
export function lightsToMeasure(devices) {
  return devices.filter((device) => device.enabled && device.profile === "classic");
}

export function measureStepIntervalCommand(device) {
  return { command: "measure_step_interval", device: device.name };
}
