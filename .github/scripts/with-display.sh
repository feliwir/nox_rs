#!/bin/sh
# Runs a command with a GTK display for the GUI tests where there is none
# (CI's container): a Broadway server, which draws into memory and needs
# no X11 or Wayland.
#
#   .github/scripts/in-linux-env.sh .github/scripts/with-display.sh cargo test …
set -eu
# Display :7 listens on broadway8.socket, in the runtime directory (else
# the cache directory).
display=7
socket="${XDG_RUNTIME_DIR:-$HOME/.cache}/broadway$((display + 1)).socket"
rm -f "$socket"
gtk4-broadwayd ":$display" >/dev/null 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null' EXIT
i=0
until [ -S "$socket" ]; do
    i=$((i + 1))
    if [ "$i" -gt 100 ]; then
        echo "gtk4-broadwayd didn't start" >&2
        exit 1
    fi
    sleep 0.1
done
GDK_BACKEND=broadway BROADWAY_DISPLAY=":$display" "$@"
