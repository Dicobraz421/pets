#!/bin/bash
# $1 = binary, $2 = out png
pkill Xvfb; sleep 0.5; Xvfb :99 -screen 0 760x360x24 >/dev/null 2>&1 & sleep 1.5
DISPLAY=:99 RUST_LOG=${RUST_LOG:-warn} timeout 10 "$1" > "${2%.png}.log" 2>&1 & sleep 7
DISPLAY=:99 import -window root "$2"; sleep 3.5
