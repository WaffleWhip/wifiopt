#!/bin/bash
set -e

# If arguments passed, execute direct one-shot command (e.g. manual testing)
if [ "$#" -gt 0 ]; then
    exec "$@"
fi

echo "[$(date)] wifiopt container starting (TZ=${TZ:-Asia/Jakarta})"

last_run_date=""

while true; do
    # Check config.json exists and is not empty
    if [ ! -s /app/config.json ]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] [cron-error] /app/config.json is missing or empty, execution disabled"
        sleep 30
        continue
    fi

    cfg_h=$(grep -o '"hour"[[:space:]]*:[[:space:]]*[0-9]*' /app/config.json | grep -o '[0-9]*' | head -n 1)
    cfg_m=$(grep -o '"minute"[[:space:]]*:[[:space:]]*[0-9]*' /app/config.json | grep -o '[0-9]*' | head -n 1)

    if [ -z "$cfg_h" ] || [ -z "$cfg_m" ]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] [cron-error] config.json missing hour or minute, execution disabled"
        sleep 30
        continue
    fi

    target_hour="$cfg_h"
    target_minute="$cfg_m"

    cur_date=$(date +%Y%m%d)
    cur_h=$(date +%H)
    cur_m=$(date +%M)

    # Base 10 integer comparison
    cur_h=$((10#$cur_h))
    cur_m=$((10#$cur_m))
    target_h=$((10#$target_hour))
    target_m=$((10#$target_minute))

    if [ "$cur_h" -eq "$target_h" ] && [ "$cur_m" -eq "$target_m" ] && [ "$last_run_date" != "$cur_date" ]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] [cron-trigger] schedule matched (${target_hour}:${target_minute} WIB) -> launching /usr/local/bin/wifiopt --date ${cur_date}"
        last_run_date="$cur_date"

        # Launch actual wifiopt binary with explicit date argument
        /usr/local/bin/wifiopt --date "$cur_date" || echo "[$(date '+%Y-%m-%d %H:%M:%S')] [cron-error] wifiopt exited with code $?"

        echo "[$(date '+%Y-%m-%d %H:%M:%S')] [cron-done] daily run finished. Memory released."
    fi

    sleep 30
done