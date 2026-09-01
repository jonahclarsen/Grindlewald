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

temporary_executable="$app_path/Contents/MacOS/grindlewald.dev-new"
/bin/cp "$built_executable" "$temporary_executable"
/bin/chmod +x "$temporary_executable"
/bin/mv -f "$temporary_executable" "$executable"
/usr/bin/codesign --force --deep --sign - \
  --identifier com.jonahclarsen.grindlewald "$app_path"

exec "$executable" "$@"
