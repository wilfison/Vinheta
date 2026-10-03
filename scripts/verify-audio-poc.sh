#!/usr/bin/env bash
# Verifies the audio proof of concept without the real microphone or headphones:
# a fake microphone (440 Hz) and a temporary sink stand in for them, and the
# sound is a 1000 Hz tone. See docs/audio-poc.md.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/audio-poc-common.sh"

mode=${1:-}
case $mode in
    shell) subject=("$root/scripts/audio-poc.sh") ;;
    rust)
        cargo build --quiet --manifest-path "$root/Cargo.toml" \
            --features audio-poc --bin vinheta-audio-poc || exit 1
        subject=("$root/target/debug/vinheta-audio-poc")
        ;;
    *) echo "usage: $0 shell|rust" >&2; exit 2 ;;
esac

work="$root/tmp/audio-poc"
mkdir -p "$work"
sound="$work/sound.wav"
mic=vinheta-test-mic
monitor=vinheta-test-monitor
present=-40
absent=-60
failures=0
pids=()
nodes=()

check() {
    if [ "$2" = ok ]; then
        echo "PASS $1"
    else
        echo "FAIL $1"
        failures=$((failures + 1))
    fi
}

# analyze level FILE CHANNEL FREQ START SECONDS: level of one frequency, in dBFS.
# analyze onset FILE CHANNEL FREQ: time in ms when that frequency first shows up.
analyze() {
    python3 - "$@" <<'PY'
import array, math, sys, wave

cmd, path, channel, freq = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
with wave.open(path) as wav:
    rate, channels = wav.getframerate(), wav.getnchannels()
    data = array.array("h", wav.readframes(wav.getnframes()))
samples = data[channel::channels]

def level(block):
    if not block:
        return -200.0
    w = 2 * math.pi * freq / rate
    re = sum(s * math.cos(w * i) for i, s in enumerate(block))
    im = sum(s * math.sin(w * i) for i, s in enumerate(block))
    amplitude = 2 * math.hypot(re, im) / len(block) / 32768
    return 20 * math.log10(max(amplitude, 1e-10))

if cmd == "level":
    start, length = int(float(sys.argv[5]) * rate), int(float(sys.argv[6]) * rate)
    print(f"{level(samples[start:start + length]):.1f}")
else:
    block, hop = rate // 200, rate // 2000
    for pos in range(0, len(samples) - block, hop):
        if level(samples[pos:pos + block]) > -40:
            print(f"{pos * 1000 / rate:.1f}")
            break
    else:
        print("none")
PY
}

louder() { python3 -c 'import sys; sys.exit(0 if float(sys.argv[1]) >= float(sys.argv[2]) else 1)' "$1" "$2"; }

record() {
    pw-record --format s16 --rate 48000 --channels 2 \
        -P node.autoconnect=false -P state.restore-props=false -P "node.name=$1" "$2" &
    pids+=($!)
    wait_for "recorder $1" port_exists "$1:input_FR"
}

leftovers() {
    node_exists vinheta || pw-link -l | grep -Eq '(^| )vinheta:'
}

gone_within_2s() {
    local i
    for i in $(seq 20); do
        leftovers || return 0
        sleep 0.1
    done
    return 1
}

defaults() { pw-metadata 0 | grep -E "default\.audio\.(source|sink)'" | sort; }

cleanup() {
    local pid node
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null; done
    wait 2>/dev/null
    for node in "${nodes[@]}"; do pw-cli destroy "$node" >/dev/null 2>&1; done
    pids=()
    nodes=()
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Plays the files (the sound by default) through the subject and records the
# call, the monitor, and a probe whose left channel is the call branch and right
# channel the monitor branch. With run_for set, the subject is not expected to
# exit: after that many seconds its state is noted and it is interrupted.
files=()
run_for=
playback() {
    local label=$1 recorders subject_pid
    shift
    [ ${#files[@]} -eq 0 ] && files=("$sound")
    "${subject[@]}" --mic "$mic" --monitor "$monitor" "$@" "${files[@]}" </dev/null >"$work/$label.log" 2>&1 &
    subject_pid=$!
    wait_for "the vinheta node" port_exists vinheta:capture_FR || return 1

    recorders=${#pids[@]}
    record vinheta-test-rec-call "$work/$label-call.wav"
    pw-link vinheta:capture_FL vinheta-test-rec-call:input_FL
    pw-link vinheta:capture_FR vinheta-test-rec-call:input_FR
    record vinheta-test-rec-monitor "$work/$label-monitor.wav"
    pw-link "$monitor:monitor_FL" vinheta-test-rec-monitor:input_FL
    pw-link "$monitor:monitor_FR" vinheta-test-rec-monitor:input_FR
    record vinheta-test-rec-probe "$work/$label-probe.wav"
    pw-link vinheta:capture_FL vinheta-test-rec-probe:input_FL
    pw-link "$monitor:monitor_FL" vinheta-test-rec-probe:input_FR

    if [ -n "$run_for" ]; then
        sleep "$run_for"
        subject_alive=no
        kill -0 "$subject_pid" 2>/dev/null && node_exists vinheta && subject_alive=ok
        kill -INT "$subject_pid" 2>/dev/null
    fi
    wait "$subject_pid"
    subject_status=$?
    files=()
    run_for=
    sleep 0.3
    kill -INT "${pids[@]:$recorders}" 2>/dev/null
    wait "${pids[@]:$recorders}" 2>/dev/null
    pids=("${pids[@]:0:$recorders}")
}

# expect LABEL FILE FREQ present|absent START [SECONDS]: checks both channels
# over SECONDS (2 by default) from START, which usually comes from window().
expect() {
    local label=$1 file=$2 freq=$3 want=$4 start=$5 length=${6:-2} channel value result
    for channel in 0 1; do
        value=$(analyze level "$file" "$channel" "$freq" "$start" "$length")
        result=fail
        if [ "$want" = present ]; then
            louder "$value" "$present" && result=ok
        else
            louder "$value" "$absent" || result=ok
        fi
        check "$label, $freq Hz $want on channel $channel ($value dBFS)" "$result"
    done
}

# window FILE [OFFSET]: OFFSET seconds (half a second by default) after the
# sound starts in the given recording, in seconds.
window() {
    local onset
    onset=$(analyze onset "$1" 0 1000)
    [ "$onset" = none ] && onset=1000
    python3 -c 'import sys; print(float(sys.argv[1]) / 1000 + float(sys.argv[2]))' "$onset" "${2:-0.5}"
}

defaults_before=$(defaults)

ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=4" \
    -af "volume=-12dB,adelay=1500:all=1,pan=stereo|c0=c0|c1=c0" -ar 48000 "$sound" || exit 1

nodes+=("$(create_node "$mic" "Vinheta test microphone" Audio/Source/Virtual MONO)")
nodes+=("$(create_node "$monitor" "Vinheta test monitor" Audio/Sink "FL FR")")
# A live GStreamer source feeding an unmanaged pipewiresink stalls the whole
# graph (docs/audio-poc.md, known issues), so the fake voice is a file.
ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=600" \
    -af "volume=-12dB" -ac 1 -ar 48000 "$work/voice.wav" || exit 1
# Without state.restore-props=false WirePlumber applies the volume it saved
# for earlier pw-play and pw-record streams.
pw-play -P node.autoconnect=false -P state.restore-props=false \
    -P node.name=vinheta-test-tone "$work/voice.wav" &
pids+=($!)
wait_for "the fake microphone tone" port_exists vinheta-test-tone:output_MONO || exit 1
pw-link vinheta-test-tone:output_MONO "$mic:input_MONO"

once=()
[ "$mode" = rust ] && once=(--once)

echo "== both branches"
playback full "${once[@]}"
check "subject exited with status 0" "$([ "$subject_status" -eq 0 ] && echo ok)"
start=$(window "$work/full-call.wav")
expect "call" "$work/full-call.wav" 440 present "$start"
expect "call" "$work/full-call.wav" 1000 present "$start"
expect "monitor" "$work/full-monitor.wav" 1000 present "$(window "$work/full-monitor.wav")"
expect "monitor" "$work/full-monitor.wav" 440 absent 1
check "nothing left after a normal exit" "$(gone_within_2s && echo ok)"

call_onset=$(analyze onset "$work/full-probe.wav" 0 1000)
monitor_onset=$(analyze onset "$work/full-probe.wav" 1 1000)
if [ "$call_onset" = none ] || [ "$monitor_onset" = none ]; then
    check "branch offset measured" fail
else
    offset=$(python3 -c 'import sys; print(f"{float(sys.argv[1]) - float(sys.argv[2]):+.1f}")' "$call_onset" "$monitor_onset")
    check "branch offset measured: call minus monitor = $offset ms" ok
fi

echo "== call volume 0"
playback call-muted "${once[@]}" --call-volume 0
start=$(window "$work/call-muted-monitor.wav")
expect "call" "$work/call-muted-call.wav" 1000 absent "$start"
expect "call" "$work/call-muted-call.wav" 440 present "$start"
expect "monitor" "$work/call-muted-monitor.wav" 1000 present "$start"

echo "== monitor volume 0"
playback monitor-muted "${once[@]}" --monitor-volume 0
start=$(window "$work/monitor-muted-call.wav")
expect "call" "$work/monitor-muted-call.wav" 1000 present "$start"
expect "monitor" "$work/monitor-muted-monitor.wav" 1000 absent "$start"

if [ "$mode" = rust ]; then
    # 8 seconds of tone, so there is room to act in the middle of it.
    long="$work/sound-long.wav"
    other="$work/sound-other.wav"
    for spec in "1000 $long" "2000 $other"; do
        ffmpeg -v error -y -f lavfi -i "sine=frequency=${spec%% *}:duration=8" \
            -af "volume=-12dB,adelay=1500:all=1,pan=stereo|c0=c0|c1=c0" -ar 48000 "${spec#* }" || exit 1
    done

    # The tone starts 1.5 s into the file, so an action 4 s after the playback
    # starts lands 2.5 s after the tone does.
    echo "== stop one sound"
    files=("$long") run_for=8
    playback stop --stop-after 4
    check "subject and node still there after the stop" "$subject_alive"
    for branch in call monitor; do
        expect "$branch before the stop" "$work/stop-$branch.wav" 1000 present "$(window "$work/stop-$branch.wav")" 1.5
        expect "$branch after the stop" "$work/stop-$branch.wav" 1000 absent "$(window "$work/stop-$branch.wav" 3.2)"
    done
    expect "call after the stop" "$work/stop-call.wav" 440 present "$(window "$work/stop-call.wav" 3.2)"

    echo "== stop all sounds"
    files=("$long" "$other") run_for=8
    playback stop-all --stop-all-after 4
    check "subject and node still there after the stop" "$subject_alive"
    for branch in call monitor; do
        for freq in 1000 2000; do
            expect "$branch before the stop" "$work/stop-all-$branch.wav" "$freq" present "$(window "$work/stop-all-$branch.wav")" 1.5
            expect "$branch after the stop" "$work/stop-all-$branch.wav" "$freq" absent "$(window "$work/stop-all-$branch.wav" 3.2)"
        done
    done

    echo "== mute and unmute the call branch"
    files=("$long")
    playback mute --once --mute-call-after 4 --unmute-call-after 6.5
    call="$work/mute-call.wav"
    expect "call before the mute" "$call" 1000 present "$(window "$call")" 1.5
    expect "call while muted" "$call" 1000 absent "$(window "$call" 3)" 1.5
    expect "call while muted" "$call" 440 present "$(window "$call" 3)" 1.5
    expect "monitor while muted" "$work/mute-monitor.wav" 1000 present "$(window "$work/mute-monitor.wav" 3)" 1.5
    expect "call after the unmute" "$call" 1000 present "$(window "$call" 5.5)" 1.5

    echo "== call branch off from the start"
    playback no-call --once --no-call
    start=$(window "$work/no-call-monitor.wav")
    expect "call" "$work/no-call-call.wav" 1000 absent "$start"
    expect "call" "$work/no-call-call.wav" 440 present "$start"
    expect "monitor" "$work/no-call-monitor.wav" 1000 present "$start"
fi

# The shell subject uses lingering nodes, so SIGKILL does not apply to it, and
# bash ignores SIGINT in background jobs, so it gets SIGTERM instead.
signals=(TERM)
[ "$mode" = rust ] && signals=(INT KILL)
for signal in "${signals[@]}"; do
    echo "== cleanup after SIG$signal"
    "${subject[@]}" --mic "$mic" --monitor "$monitor" "$sound" </dev/null >"$work/sig$signal.log" 2>&1 &
    subject_pid=$!
    if wait_for "the vinheta node" port_exists vinheta:capture_FR && sleep 2.5; then
        check "microphone linked before SIG$signal" \
            "$(pw-link -l | grep -A2 "^$mic:capture_MONO" | grep -q 'vinheta:input_FL' && echo ok)"
        kill "-$signal" "$subject_pid"
        check "nothing left after SIG$signal" "$(gone_within_2s && echo ok)"
    else
        check "subject started for SIG$signal" fail
    fi
    wait "$subject_pid" 2>/dev/null
done

cleanup
sleep 0.3
stray=$(pw-dump | grep -c '"node.name": "vinheta' || true)
check "no vinheta node left ($stray found)" "$([ "$stray" -eq 0 ] && echo ok)"
check "default source and sink unchanged" "$([ "$(defaults)" = "$defaults_before" ] && echo ok)"

if [ "$failures" -eq 0 ]; then
    echo "ALL CHECKS PASSED"
else
    echo "$failures CHECK(S) FAILED"
    exit 1
fi
