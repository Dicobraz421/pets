#!/bin/bash
pkill Xvfb; sleep 0.5; Xvfb :98 -screen 0 800x400x24 >/dev/null 2>&1 & XV=$!; sleep 1.5
export DISPLAY=:98 RUST_LOG=${RUST_LOG:-gpui_linux=debug}
${BIN:-./target/debug/dnd} dst > dst.log 2>&1 & D=$!
${BIN:-./target/debug/dnd} src > src.log 2>&1 & S=$!
sleep 6
xdotool mousemove 50 50; sleep 0.3; xdotool mousedown 1; sleep 0.3
for x in 60 80 120 200 280 320 360 420 480 500; do xdotool mousemove $x 60; sleep 0.25; done
import -window root dnd_mid.png
xdotool mouseup 1; sleep 2
kill $D $S $XV
