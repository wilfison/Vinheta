#!/usr/bin/env bash
# Command line proof of concept: creates the "Vinheta" virtual microphone,
# links a microphone into it, and plays a file to the call and to the monitor.
# See docs/audio-poc.md.
set -euo pipefail

. "$(dirname "$0")/audio-poc-common.sh"

usage() {
    echo "usage: $0 [--mic NODE_NAME] [--monitor NODE_NAME] [--call-volume N] [--monitor-volume N] FILE" >&2
    exit 2
}

mic=
monitor=
call_volume=1.0
monitor_volume=1.0
file=
while [ $# -gt 0 ]; do
    case $1 in
        --mic) mic=${2:?}; shift 2 ;;
        --monitor) monitor=${2:?}; shift 2 ;;
        --call-volume) call_volume=${2:?}; shift 2 ;;
        --monitor-volume) monitor_volume=${2:?}; shift 2 ;;
        -*) usage ;;
        *) file=$1; shift ;;
    esac
done
[ -n "$file" ] && [ -f "$file" ] || usage

if node_exists vinheta; then
    echo "a node named \"vinheta\" already exists" >&2
    exit 1
fi

node=
gst_pid=
cleanup() {
    [ -n "$gst_pid" ] && kill "$gst_pid" 2>/dev/null || true
    [ -n "$node" ] && pw-cli destroy "$node" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

node=$(create_node vinheta Vinheta Audio/Source/Virtual "FL FR")
echo "created node $node"

mic=${mic:-$(default_source)}
if [ "$mic" = vinheta ] || [ -z "$mic" ]; then
    echo "no microphone to link" >&2
else
    link_to_stereo "$mic" vinheta capture input
    echo "linked microphone $mic"
fi

# WirePlumber does not route a playback stream to an Audio/Source/Virtual node,
# so the call branch does not autoconnect and is linked by hand.
stream="vinheta-call-$$"
gst-launch-1.0 -q \
    uridecodebin uri="file://$(realpath "$file")" ! audioconvert ! audioresample ! tee name=t \
    t. ! queue ! volume volume="$call_volume" \
       ! pipewiresink stream-properties="p,node.autoconnect=false,node.name=$stream" \
    t. ! queue ! volume volume="$monitor_volume" \
       ! pipewiresink ${monitor:+target-object="$monitor"} &
gst_pid=$!

wait_for "playback stream ports" port_exists "$stream:output_FR"
pw-link "$stream:output_FL" vinheta:input_FL
pw-link "$stream:output_FR" vinheta:input_FR

wait "$gst_pid"
gst_pid=
