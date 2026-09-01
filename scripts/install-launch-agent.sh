#!/bin/zsh
set -eu

label="com.jonahclarsen.grindlewald-dev"
script_dir="${0:A:h}"
repo_dir="${script_dir:h}"
pnpm_bin="$(command -v pnpm)"
agent_dir="$HOME/Library/LaunchAgents"
log_dir="$HOME/Library/Logs/Grindlewald"
agent_path="$agent_dir/$label.plist"
template="$script_dir/$label.plist.template"
temporary="$(mktemp)"

mkdir -p "$agent_dir" "$log_dir"
sed \
  -e "s|__RUNNER__|$script_dir/run-dev.sh|g" \
  -e "s|__REPO__|$repo_dir|g" \
  -e "s|__PNPM__|$pnpm_bin|g" \
  -e "s|__LOG_DIR__|$log_dir|g" \
  "$template" > "$temporary"
plutil -lint "$temporary"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
cp "$temporary" "$agent_path"
chmod 600 "$agent_path"
launchctl bootstrap "gui/$(id -u)" "$agent_path"
launchctl enable "gui/$(id -u)/$label"
launchctl kickstart -k "gui/$(id -u)/$label"

echo "Installed and started $label"
