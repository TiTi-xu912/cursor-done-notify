#!/bin/bash
# Unzips the macOS package into a scratch HOME, installs, runs a hook the way Cursor would,
# and uninstalls. The user's own ~/.cursor is never touched.
set -euo pipefail
zip="$1"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
ditto -x -k "$zip" "$scratch"
pkg="$(find "$scratch" -maxdepth 1 -type d -name 'cursor-done-notify-*' | head -1)"

export HOME="$scratch/home"
export TMPDIR="$scratch/tmp/"
mkdir -p "$HOME/.cursor" "$TMPDIR"
hooks="$HOME/.cursor/hooks.json"
printf '%s\n' '{"version":1,"hooks":{"stop":[{"command":"/keep/other-hook.sh","timeout":9}]}}' > "$hooks"

bash "$pkg/install.sh" --realtime
cat "$hooks"
grep -q '/keep/other-hook.sh' "$hooks"
grep -q "hook-relay' allow" "$hooks"
[ -f "$hooks.bak" ]

# Run our stop hook through a shell, exactly as written in hooks.json.
cmd="$(plutil -extract hooks.stop.1.command raw -o - "$hooks")"
reply="$(printf '%s' '{"hook_event_name":"stop","conversation_id":"install-test"}' | sh -c "$cmd")"
[ "$reply" = "{}" ] || { echo "relay reply: $reply" >&2; exit 1; }
sleep 2
pgrep -f "$pkg/buddy-host" > /dev/null || { echo "the relay did not start the host" >&2; exit 1; }

bash "$pkg/uninstall.sh"
cat "$hooks"
if grep -q 'hook-relay' "$hooks"; then echo "uninstall left a relay hook" >&2; exit 1; fi
grep -q '/keep/other-hook.sh' "$hooks"
sleep 1
if pgrep -f "$pkg/buddy-host" > /dev/null; then echo "uninstall did not stop the host" >&2; exit 1; fi

echo "MAC INSTALL TEST OK"
