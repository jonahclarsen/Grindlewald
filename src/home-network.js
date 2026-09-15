export function homeNetworkMessage(schedule, currentNetwork, state) {
  if (state === "loading") return "Checking the current network…";
  if (state === "error") return "Could not check the home network. Open Automations or click the option to try again.";
  if (!currentNetwork) return "No Wi-Fi router detected. Connect to your home Wi-Fi, then click this option to save it.";
  if (schedule.floodlights !== "on_home_network" || !schedule.floodlightNetwork) {
    return "Click to save the current network as home. Location Services can stay off.";
  }
  return schedule.floodlightNetwork === currentNetwork
    ? "On your saved home network. Click again to save the current network."
    : "Away from your saved home network. Click again to replace it with the current network.";
}
