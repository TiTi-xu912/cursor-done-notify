#!/bin/bash
# Stops Diandian and removes only its entries from ~/.cursor/hooks.json.
set -euo pipefail
cd "$(dirname "$0")"
exec ./buddy-host --uninstall
