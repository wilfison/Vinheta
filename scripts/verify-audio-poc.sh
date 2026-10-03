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
mic2=vinheta-test-mic2
monitor2=vinheta-test-monitor2
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
record_monitor2=
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
    if [ -n "$record_monitor2" ]; then
        record vinheta-test-rec-monitor2 "$work/$label-monitor2.wav"
        pw-link "$monitor2:monitor_FL" vinheta-test-rec-monitor2:input_FL
        pw-link "$monitor2:monitor_FR" vinheta-test-rec-monitor2:input_FR
    fi
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
    record_monitor2=
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

# drop FILE FREQ FIRST SECOND [SECONDS]: how many dB the level of the left
# channel falls from the window at FIRST to the window at SECOND.
drop() {
    python3 -c 'import sys; print(f"{float(sys.argv[1]) - float(sys.argv[2]):.1f}")' \
        "$(analyze level "$1" 0 "$2" "$3" "${5:-1.5}")" "$(analyze level "$1" 0 "$2" "$4" "${5:-1.5}")"
}

# between VALUE LOW HIGH
between() {
    python3 -c 'import sys; v, low, high = map(float, sys.argv[1:]); sys.exit(0 if low <= v <= high else 1)' "$@" && echo ok
}

# The links of the first port of the engine's monitor stream, and of one
# input of the virtual microphone.
monitor_links() { pw-link -l | grep -A2 '^vinheta-monitor-[0-9-]*:output_FL'; }
voice_links() { pw-link -l | grep -A4 "^vinheta:input_$1" | grep '|<-' | grep -v 'vinheta-call-'; }

# device_blocks LOG: the "devices changed" blocks of a log, one per line.
device_blocks() {
    awk '/^devices changed$/ { if (block) print block; block = "#"; next }
         /^(microphone|output): / { if (block) block = block " " $0; next }
         END { if (block) print block }' "$1"
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

    # The gain changes 2.5 s after the tone starts.
    for branch in call monitor; do
        other=monitor
        [ "$branch" = monitor ] && other=call
        echo "== $branch volume change"
        files=("$long")
        playback "$branch-volume" --once "--$branch-volume-after" 4 0.1
        changed="$work/$branch-volume-$branch.wav"
        kept="$work/$branch-volume-$other.wav"
        value=$(drop "$changed" 1000 "$(window "$changed")" "$(window "$changed" 3.2)")
        check "$branch falls by 20 dB at gain 0.1 ($value dB)" "$(between "$value" 17 23)"
        value=$(drop "$changed" 1000 "$(window "$changed")" "$(window "$changed" 2.7)" 0.5)
        check "$branch has fallen 200 ms after the change ($value dB)" "$(between "$value" 17 23)"
        value=$(drop "$kept" 1000 "$(window "$kept")" "$(window "$kept" 3.2)")
        check "$other keeps its level ($value dB)" "$(between "$value" -2 2)"
        call="$work/$branch-volume-call.wav"
        value=$(drop "$call" 440 "$(window "$call")" "$(window "$call" 3.2)")
        check "the voice keeps its level ($value dB)" "$(between "$value" -2 2)"
    done

    echo "== device list"
    "${subject[@]}" --mic "$mic" --monitor "$monitor" </dev/null >"$work/devices.log" 2>&1 &
    subject_pid=$!
    if wait_for "the vinheta node" port_exists vinheta:capture_FR; then
        sleep 0.5
        extra=$(create_node vinheta-test-extra "Vinheta test extra" Audio/Sink "FL FR")
        sleep 2
        with_extra=$(device_blocks "$work/devices.log" | tail -n 1)
        pw-cli destroy "$extra" >/dev/null
        sleep 2
    fi
    kill -INT "$subject_pid" 2>/dev/null
    wait "$subject_pid" 2>/dev/null
    first=$(device_blocks "$work/devices.log" | head -n 1)
    last=$(device_blocks "$work/devices.log" | tail -n 1)
    check "the fake microphone is listed with its description" \
        "$([[ $first == *"microphone: $mic (Vinheta test microphone)"* ]] && echo ok)"
    check "the fake sink is listed with its description" \
        "$([[ $first == *"output: $monitor (Vinheta test monitor)"* ]] && echo ok)"
    check "the virtual microphone is never listed" \
        "$(grep -q '^microphone: vinheta (' "$work/devices.log" || echo ok)"
    check "a sink plugged in shows up within 2 s" \
        "$([[ $with_extra == *"output: vinheta-test-extra (Vinheta test extra)"* ]] && echo ok)"
    check "a removed sink leaves the list" "$([[ $last != *vinheta-test-extra* ]] && echo ok)"

    echo "== voice off and on"
    files=("$long")
    playback voice --once --voice-off-after 4 --voice-on-after 6.5
    call="$work/voice-call.wav"
    expect "call before the voice is off" "$call" 440 present "$(window "$call")" 1.5
    expect "call while the voice is off" "$call" 440 absent "$(window "$call" 3)" 1.5
    expect "call while the voice is off" "$call" 1000 present "$(window "$call" 3)" 1.5
    expect "call after the voice is back" "$call" 440 present "$(window "$call" 5.5)" 1.5

    echo "== voice off from the start"
    playback no-voice --once --no-voice
    start=$(window "$work/no-voice-call.wav")
    expect "call" "$work/no-voice-call.wav" 440 absent "$start"
    expect "call" "$work/no-voice-call.wav" 1000 present "$start"

    # A second fake microphone, with another tone, and a second fake sink.
    mic2_id=$(create_node "$mic2" "Vinheta test microphone 2" Audio/Source/Virtual MONO)
    monitor2_id=$(create_node "$monitor2" "Vinheta test monitor 2" Audio/Sink "FL FR")
    nodes+=("$mic2_id" "$monitor2_id")
    ffmpeg -v error -y -f lavfi -i "sine=frequency=880:duration=600" \
        -af "volume=-12dB" -ac 1 -ar 48000 "$work/voice2.wav" || exit 1
    pw-play -P node.autoconnect=false -P state.restore-props=false \
        -P node.name=vinheta-test-tone2 "$work/voice2.wav" &
    pids+=($!)
    wait_for "the second fake microphone tone" port_exists vinheta-test-tone2:output_MONO || exit 1
    pw-link vinheta-test-tone2:output_MONO "$mic2:input_MONO"

    echo "== microphone switch"
    files=("$long")
    playback mic-switch --once --mic-after 4 "$mic2"
    call="$work/mic-switch-call.wav"
    expect "call before the switch" "$call" 440 present "$(window "$call")" 1.5
    expect "call before the switch" "$call" 880 absent "$(window "$call")" 1.5
    expect "call after the switch" "$call" 440 absent "$(window "$call" 3.2)" 1.5
    expect "call after the switch" "$call" 880 present "$(window "$call" 3.2)" 1.5
    expect "call after the switch" "$call" 1000 present "$(window "$call" 3.2)" 1.5

    echo "== monitor switch"
    files=("$long") record_monitor2=1
    playback monitor-switch --once --monitor-after 4 "$monitor2"
    first="$work/monitor-switch-monitor.wav"
    second="$work/monitor-switch-monitor2.wav"
    call="$work/monitor-switch-call.wav"
    expect "first sink before the switch" "$first" 1000 present "$(window "$first")" 1.5
    expect "first sink 1 s after the switch" "$first" 1000 absent "$(window "$first" 3.5)" 1.5
    expect "second sink after the switch" "$second" 1000 present "$(window "$second")" 1.5
    # The tone reaches the second sink when the stream is moved, 2.5 s after
    # it reached the first one.
    moved=$(python3 -c 'import sys; print(f"{(float(sys.argv[2]) - float(sys.argv[1])) / 1000:.2f}")' \
        "$(analyze onset "$first" 0 1000)" "$(analyze onset "$second" 0 1000)" 2>/dev/null)
    check "the stream moved within 1 s of the request ($moved s after the tone started)" "$(between "${moved:-0}" 2 3.5)"
    expect "call before the switch" "$call" 1000 present "$(window "$call")" 1.5
    expect "call during the switch" "$call" 1000 present "$(window "$call" 2.2)" 1
    expect "call after the switch" "$call" 1000 present "$(window "$call" 3.2)" 1.5

    # The output changes before the tone of the first playback starts, and
    # the files are played again after it ended. Without --once, which would
    # exit before that.
    echo "== next sound uses the new output"
    record_monitor2=1 run_for=12
    playback next-output --monitor-after 0.5 "$monitor2" --replay-after 6.5
    first="$work/next-output-monitor.wav"
    second="$work/next-output-monitor2.wav"
    check "nothing reaches the first sink" "$([ "$(analyze onset "$first" 0 1000)" = none ] && echo ok)"
    expect "second sink, first playback" "$second" 1000 present "$(window "$second")"
    expect "second sink, second playback" "$second" 1000 present "$(window "$second" 7)"

    # The fallback of a removed output is the real default sink, so this one
    # plays with the monitor volume at 0.
    echo "== output removed while playing"
    (
        sleep 4.5
        monitor_links >"$work/output-removed-before.txt"
        pw-cli destroy "$monitor2_id" >/dev/null
        sleep 1.5
        monitor_links >"$work/output-removed-after.txt"
    ) &
    watcher=$!
    files=("$long")
    playback output-removed --once --monitor "$monitor2" --monitor-volume 0
    wait "$watcher"
    check "the monitor stream was on the chosen sink" \
        "$(grep -q "$monitor2:playback_FL" "$work/output-removed-before.txt" && echo ok)"
    check "the monitor stream moved to another sink" \
        "$(grep '|->' "$work/output-removed-after.txt" | grep -qv "$monitor2:" && echo ok)"
    check "subject exited with status 0" "$([ "$subject_status" -eq 0 ] && echo ok)"
    call="$work/output-removed-call.wav"
    expect "call after the removal" "$call" 1000 present "$(window "$call" 4.5)" 1.5

    # The fallback of a removed microphone is the real default source, so
    # this one plays nothing and records nothing.
    echo "== microphone removed and back"
    "${subject[@]}" --mic "$mic2" --monitor "$monitor" </dev/null >"$work/mic-removed.log" 2>&1 &
    subject_pid=$!
    if wait_for "the vinheta node" port_exists vinheta:capture_FR && sleep 1.5; then
        check "the chosen microphone is linked" "$(voice_links FL | grep -q "$mic2:" && echo ok)"
        pw-cli destroy "$mic2_id" >/dev/null
        sleep 2
        check "the missing microphone is reported" \
            "$(grep -q "microphone \"$mic2\" not found" "$work/mic-removed.log" && echo ok)"
        check "another microphone is linked as a fallback" \
            "$(grep -q '^microphone linked as a fallback: ' "$work/mic-removed.log" && echo ok)"
        check "the fallback feeds both inputs" \
            "$(voice_links FL | grep -q . && voice_links FR | grep -q . && echo ok)"
        mic2_id=$(create_node "$mic2" "Vinheta test microphone 2" Audio/Source/Virtual MONO)
        nodes+=("$mic2_id")
        sleep 2
        check "the chosen microphone is linked again" \
            "$(tail -n 3 "$work/mic-removed.log" | grep -q "^microphone linked: $mic2" && voice_links FL | grep -q "$mic2:" && echo ok)"
    else
        check "subject started for the microphone removal" fail
    fi
    kill -INT "$subject_pid" 2>/dev/null
    wait "$subject_pid" 2>/dev/null
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
