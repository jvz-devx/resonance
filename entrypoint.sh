#!/bin/sh
# Generate yt-dlp config from environment variable
printf '%s\n' \
  '--extractor-args' \
  "youtubepot-bgutilhttp:base_url=${POT_SERVER_URL}" \
  > /etc/yt-dlp.conf

exec resonance "$@"
