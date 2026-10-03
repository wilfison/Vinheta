#!/usr/bin/env bash
# Builds, installs into the local prefix (build/install), and runs the app.
# Arguments after the options go to the app.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"

if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then
    echo "usage: run-dev.sh [--debug] [APP ARGUMENTS]"
    echo "--debug  prints the app's debug messages (engine events, playback start times)"
    exit 0
fi
if [ "${1:-}" = --debug ]; then
    export G_MESSAGES_DEBUG=vinheta
    shift
fi

install_app || exit 1
app_env
exec "$prefix/bin/vinheta" "$@"
