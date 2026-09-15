export function wifiStatusLabel(state) {
  return {
    permission_needed: "Location permission needed",
    permission_denied: "Location permission denied",
    location_disabled: "Location Services off",
    restricted: "Location access restricted",
    not_connected: "No Wi-Fi network detected",
  }[state] || "Unavailable";
}

export function wifiStatusMessage(state) {
  return {
    connected: "",
    permission_needed: "Allow Grindlewald’s Location request to read your Wi-Fi name. macOS requires this permission for Wi-Fi automations.",
    permission_denied: "Enable Grindlewald in System Settings → Privacy & Security → Location Services, then return here.",
    location_disabled: "Turn on Location Services in System Settings → Privacy & Security → Location Services, then return here.",
    restricted: "Location access is restricted on this Mac. Grindlewald cannot read your Wi-Fi name.",
    not_connected: "No Wi-Fi network name is available. Check your Wi-Fi connection and Location permission for Grindlewald.",
  }[state] ?? "Could not read Wi-Fi information. Try opening Automations again.";
}
