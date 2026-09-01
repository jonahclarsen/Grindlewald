#!/bin/zsh
set -eu

label="com.jonahclarsen.grindlewald-dev"
agent_path="$HOME/Library/LaunchAgents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
if [[ -f "$agent_path" ]]; then
  mv "$agent_path" "$HOME/.Trash/$label.plist"
fi
echo "Stopped $label and moved its LaunchAgent plist to the Trash"
