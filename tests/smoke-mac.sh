#!/bin/bash
# Starts the macOS host, feeds one turn through hook-relay, and records what happened:
# fonts in use, the live state, screenshots, and resident memory. Run after `cargo build --release`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/proto/buddy-rs/target/release"
out="${1:-${TMPDIR%/}/buddy-smoke}"
mkdir -p "$out"
state="${TMPDIR%/}/cursor-done-notify"
rm -rf "$state"

"$bin/buddy-host" --fonts | tee "$out/fonts.txt"

"$bin/buddy-host" --stay --exit-after 25 &
host=$!
sleep 3
kill -0 "$host"

send() { printf '%s' "$1" | "$bin/hook-relay" "${2:-empty}" > /dev/null; }
turn='"conversation_id":"smoke","generation_id":"g1"'

send '{"hook_event_name":"beforeSubmitPrompt",'"$turn"',"prompt":"把点点搬到 Mac 上","workspace_roots":["'"$root"'"]}' continue
send '{"hook_event_name":"postToolUse",'"$turn"',"tool_name":"Read","tool_use_id":"t1","file_path":"'"$root"'/proto/buddy-rs/src/mac.rs"}'
send '{"hook_event_name":"preToolUse",'"$turn"',"tool_name":"Shell","tool_use_id":"t2","command":"cargo test"}' allow
sleep 2
cat "$state/live-view.txt"; echo
grep -q '^running|2|' "$state/live-view.txt"
screencapture -x "$out/running.png" || echo "screencapture failed"
ps -o rss= -p "$host" | awk '{ printf "resident %.1f MB\n", $1 / 1024 }' | tee "$out/memory.txt"

send '{"hook_event_name":"postToolUse",'"$turn"',"tool_name":"Shell","tool_use_id":"t2"}'
send '{"hook_event_name":"afterAgentResponse",'"$turn"',"text":"Mac 窗口层已经接上：透明置顶面板、菜单栏图标、系统音都在。"}'
send '{"hook_event_name":"stop",'"$turn"',"status":"completed"}'
sleep 2
cat "$state/live-view.txt"; echo
grep -q '^done|' "$state/live-view.txt"
screencapture -x "$out/done.png" || echo "screencapture failed"
kill -0 "$host"

# --exit-after ends the host; a crash would make wait fail.
wait "$host"
echo "MAC SMOKE OK"
