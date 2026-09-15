#!/bin/zsh
set -eu

script_dir="${0:A:h}"
app_path="$HOME/Applications/Grindlewald.app"
executable="$app_path/Contents/MacOS/grindlewald"

if [[ "${1:-}" != "__launch" ]]; then
  rustc_bin="${GRINDLEWALD_RUSTC:?GRINDLEWALD_RUSTC is not set}"
  cargo_bin="${GRINDLEWALD_CARGO:?GRINDLEWALD_CARGO is not set}"
  host="$($rustc_bin -vV | sed -n 's/^host: //p')"
  runner_config="target.'$host'.runner = ['$script_dir/dev-app-runner.sh', '__launch']"
  exec "$cargo_bin" --config "$runner_config" "$@"
fi

shift
built_executable="${1:?Cargo did not provide the built executable path}"
shift

if [[ ! -d "$app_path/Contents/MacOS" ]]; then
  echo "Grindlewald.app is not installed; run ./scripts/install-launch-agent.sh" >&2
  exit 1
fi

source "$script_dir/dev-signing.sh"
resolve_dev_signing_identity

temporary_executable="$app_path/Contents/MacOS/grindlewald.dev-new"
/bin/cp "$built_executable" "$temporary_executable"
/bin/chmod +x "$temporary_executable"
/bin/mv -f "$temporary_executable" "$executable"
# Remove permission descriptions left in installed bundles by the old SSID lookup.
for key in NSLocationUsageDescription NSLocationWhenInUseUsageDescription NSLocationAlwaysAndWhenInUseUsageDescription; do
  if /usr/bin/plutil -extract "$key" raw -o /dev/null "$app_path/Contents/Info.plist" 2>/dev/null; then
    /usr/bin/plutil -remove "$key" "$app_path/Contents/Info.plist"
  fi
done
sign_dev_app "$app_path"

exec "$executable" "$@"
