#!/bin/zsh
set -eu

label="com.jonahclarsen.grindlewald-dev"
agent_path="$HOME/Library/LaunchAgents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
if [[ -f "$agent_path" ]]; then
  mv "$agent_path" "$HOME/.Trash/$label.plist"
fi
app_path="$HOME/Applications/Grindlewald.app"
if [[ -d "$app_path" ]]; then
  destination="$HOME/.Trash/Grindlewald-$(date +%Y%m%d-%H%M%S).app"
  mv "$app_path" "$destination"
fi
echo "Stopped $label and moved its LaunchAgent plist and installed app to the Trash"
