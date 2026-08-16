#!/bin/bash
set -e

echo "[$(date)] wifiopt container starting (TZ=${TZ:-unset})"

# Launch wifiopt in loop mode (scheduler reads config.json each iteration).
exec /usr/local/bin/wifiopt --loop