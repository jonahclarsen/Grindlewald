#!/bin/zsh
set -eu

label="com.jonahclarsen.grindlewald-dev"
script_dir="${0:A:h}"
repo_dir="${script_dir:h}"
pnpm_bin="$(command -v pnpm)"
cargo_bin="$(command -v cargo)"
rustc_bin="$(command -v rustc)"
app_source="$repo_dir/src-tauri/target/debug/bundle/macos/Grindlewald.app"
app_dir="$HOME/Applications"
app_path="$app_dir/Grindlewald.app"
executable="$app_path/Contents/MacOS/grindlewald"
agent_dir="$HOME/Library/LaunchAgents"
log_dir="$HOME/Library/Logs/Grindlewald"
agent_path="$agent_dir/$label.plist"
template="$script_dir/$label.plist.template"
temporary="$(mktemp)"

echo "Building the branded debug app bundle…"
cd "$repo_dir"
pnpm tauri build --debug --bundles app

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
mkdir -p "$app_dir" "$agent_dir" "$log_dir"
/usr/bin/ditto "$app_source" "$app_path"
/usr/bin/codesign --force --deep --sign - \
  --identifier com.jonahclarsen.grindlewald "$app_path"

sed \
  -e "s|__EXECUTABLE__|$executable|g" \
  -e "s|__REPO__|$repo_dir|g" \
  -e "s|__PNPM__|$pnpm_bin|g" \
  -e "s|__CARGO__|$cargo_bin|g" \
  -e "s|__RUSTC__|$rustc_bin|g" \
  -e "s|__PATH__|${cargo_bin:h}:${pnpm_bin:h}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin|g" \
  -e "s|__LOG_DIR__|$log_dir|g" \
  "$template" > "$temporary"
plutil -lint "$temporary"

cp "$temporary" "$agent_path"
chmod 600 "$agent_path"
launchctl bootstrap "gui/$(id -u)" "$agent_path"
launchctl enable "gui/$(id -u)/$label"
launchctl kickstart -k "gui/$(id -u)/$label"

echo "Installed and started Grindlewald in live development mode"
