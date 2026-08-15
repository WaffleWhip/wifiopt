#!/bin/bash
set -e

echo "[$(date)] wifiopt container starting (TZ=$TZ)"

# Print effective cron schedule
echo "[$(date)] cron schedule:"
crontab -l

# Launch cron in foreground (keeps container alive)
exec cron -f