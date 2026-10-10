#!/usr/bin/env bash
# Verifies the audio engine without the real microphone, headphones, or call
# apps: a fake microphone (440 Hz), a temporary sink, and recorders posing as
# call apps stand in for them, and the sound is a 1000 Hz tone. See
# docs/audio.md.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/audio-common.sh"

usage() {
    echo "usage: $0 [--only REGEX] [--list]
--only REGEX  runs only the sections whose title matches (grep -E), for example
              --only 'loop|fade'; the setup and the final checks always run
--list        prints the section titles and exits" >&2
    exit 2
}

only=
while [ $# -gt 0 ]; do
    case $1 in
        --only) [ $# -ge 2 ] || usage; only=$2; shift ;;
        --list) sed -n 's/^ *if section "\(.*\)"; then$/\1/p' "$0"; exit 0 ;;
        *) usage ;;
    esac
    shift
done
cargo build --quiet --manifest-path "$root/Cargo.toml" \
    --features audio-test --bin vinheta-audio-test || exit 1
subject=("$root/target/debug/vinheta-audio-test")

work="$root/tmp/audio"
mkdir -p "$work"
rm -f "$work/onset-failures"
sound="$work/sound.wav"
mic=vinheta-test-mic
monitor=vinheta-test-monitor
mic2=vinheta-test-mic2
monitor2=vinheta-test-monitor2
# The fake call apps, as the engine names them. Every playback is sent to
# the first one only: with every app as the target, the tones would reach
# the real apps that are recording.
app=vinheta-test-call
other_app=vinheta-test-other
present=-40
absent=-60
failures=0
pids=()
nodes=()

# section TITLE: announces a section, or skips it when --only does not match.
section() {
    if [ -n "$only" ] && ! grep -Eq -- "$only" <<<"$1"; then
        return 1
    fi
    echo "== $1"
}

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
# analyze gaps FILE CHANNEL START END: the longest silence (below -50 dBFS)
# between two times in seconds, in ms.
# analyze span FILE CHANNEL: seconds from the first sound to the last one.
# analyze peak FILE CHANNEL START SECONDS: the largest sample, in dBFS.
analyze() {
    python3 - "$@" <<'PY'
import array, math, sys, wave

cmd, path, channel = sys.argv[1], sys.argv[2], int(sys.argv[3])
freq = float(sys.argv[4]) if cmd in ("level", "onset") else 0
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

def loud(start, end, block):
    # Whether each block between two sample positions is above -50 dBFS (RMS).
    for pos in range(start, min(end, len(samples)) - block + 1, block):
        yield math.sqrt(sum(s * s for s in samples[pos:pos + block]) / block) > 32768 * 10 ** (-50 / 20)

if cmd == "level":
    start, length = int(float(sys.argv[5]) * rate), int(float(sys.argv[6]) * rate)
    print(f"{level(samples[start:start + length]):.1f}")
elif cmd == "gaps":
    block, longest, run = rate // 200, 0, 0
    for sound in loud(int(float(sys.argv[4]) * rate), int(float(sys.argv[5]) * rate), block):
        run = 0 if sound else run + 1
        longest = max(longest, run)
    print(longest * 5)
elif cmd == "peak":
    start, length = int(float(sys.argv[4]) * rate), int(float(sys.argv[5]) * rate)
    peak = max((abs(s) for s in samples[start:start + length]), default=0)
    print(f"{20 * math.log10(max(peak, 1) / 32768):.1f}")
elif cmd == "span":
    block = rate // 100
    sounds = [i for i, sound in enumerate(loud(0, len(samples), block)) if sound]
    print(f"{(sounds[-1] - sounds[0] + 1) / 100:.2f}" if sounds else "0")
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

# call_app APP FILE [MIC]: a recorder that WirePlumber links to the fake
# microphone, as it does with a call app.
call_app() {
    pw-record --format s16 --rate 48000 --channels 2 --target "${3:-$mic}" \
        -P "{ state.restore-props=false node.name=$1 application.name=\"Vinheta test app $1\" application.process.binary=$1 }" "$2" &
    pids+=($!)
    wait_for "the app $1" fed "$1"
}

# Whether a microphone feeds the recorder, and whether a sound does.
fed() { pw-link -l | grep -A1 "^$1:input_" | grep -q '|<-'; }
call_linked() { pw-link -l | grep -A4 "^$1:input_" | grep -q '|<- vinheta-call-'; }

ready() { grep -q '^engine ready' "$1"; }

leftovers() {
    pw-dump | grep -Eq '"node.name": "vinheta-(drain|call|monitor)-'
}

gone_within_2s() {
    local i
    for i in $(seq 20); do
        leftovers || return 0
        sleep 0.1
    done
    return 1
}

# The node names of the engine's playback streams.
streams() { pw-dump | grep -oE '"node.name": "vinheta-(call|monitor)-[0-9-]+"'; }

defaults() { pw-metadata 0 | grep -E "default\.audio\.(source|sink)'" | sort; }

cleanup() {
    local pid node
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null; done
    wait 2>/dev/null
    for node in "${nodes[@]}"; do pw-cli destroy "$node" >/dev/null 2>&1; done
    [ -n "${private_socket:-}" ] && rm -f "$private_socket" "$private_socket.lock" \
        "$private_socket-manager" "$private_socket-manager.lock"
    pids=()
    nodes=()
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Plays the files (the sound by default) through the subject and records the
# call (what the fake call app records), the monitor, and a probe whose left
# channel is the call branch as the drain node gets it and right channel the
# monitor branch. With run_for set, the subject is not expected to exit:
# after that many seconds its state is noted and it is interrupted.
# call_mic is the microphone of the call app, no_call_app leaves the app out,
# and with_other_app adds a second one.
files=()
run_for=
subject_limit=60
record_monitor2=
call_mic=
no_call_app=
with_other_app=
playback() {
    local label=$1 recorders subject_pid
    shift
    [ ${#files[@]} -eq 0 ] && files=("$sound")
    subject_started=$(date +%s.%N)
    "${subject[@]}" --target "$app" --monitor "$monitor" "$@" "${files[@]}" </dev/null >"$work/$label.log" 2>&1 &
    subject_pid=$!
    wait_for "the engine" ready "$work/$label.log" || return 1

    recorders=${#pids[@]}
    [ -z "$no_call_app" ] && call_app "$app" "$work/$label-call.wav" "${call_mic:-$mic}"
    [ -n "$with_other_app" ] && call_app "$other_app" "$work/$label-other.wav"
    record vinheta-test-rec-monitor "$work/$label-monitor.wav"
    pw-link "$monitor:monitor_FL" vinheta-test-rec-monitor:input_FL
    pw-link "$monitor:monitor_FR" vinheta-test-rec-monitor:input_FR
    if [ -n "$record_monitor2" ]; then
        record vinheta-test-rec-monitor2 "$work/$label-monitor2.wav"
        pw-link "$monitor2:monitor_FL" vinheta-test-rec-monitor2:input_FL
        pw-link "$monitor2:monitor_FR" vinheta-test-rec-monitor2:input_FR
    fi
    record vinheta-test-rec-probe "$work/$label-probe.wav"
    pw-link "vinheta-drain-$subject_pid:monitor_MONO" vinheta-test-rec-probe:input_FL
    pw-link "$monitor:monitor_FL" vinheta-test-rec-probe:input_FR

    if [ -n "$run_for" ]; then
        sleep "$run_for"
        subject_alive=no
        kill -0 "$subject_pid" 2>/dev/null && node_exists "vinheta-drain-$subject_pid" && subject_alive=ok
        kill -INT "$subject_pid" 2>/dev/null
    fi
    # No playback of the harness lasts a minute. A subject that does is
    # stuck, as when the PipeWire graph stops scheduling the nodes, and so
    # would every section after it. A short loop also lets a TERM through.
    local deadline=$((SECONDS + subject_limit))
    while kill -0 "$subject_pid" 2>/dev/null && [ "$SECONDS" -lt "$deadline" ]; do
        sleep 0.2
    done
    if kill -0 "$subject_pid" 2>/dev/null; then
        check "$label: the subject ends within $subject_limit s (a stuck graph? see pw-top)" fail
        kill -KILL "$subject_pid" 2>/dev/null
        echo "aborted: the rest of the harness would be stuck too" >&2
        exit 1
    fi
    wait "$subject_pid"
    subject_status=$?
    subject_seconds=$(python3 -c 'import sys, time; print(f"{time.time() - float(sys.argv[1]):.1f}")' "$subject_started")
    files=()
    run_for=
    record_monitor2=
    call_mic=
    no_call_app=
    with_other_app=
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
    # A guessed onset would put every measurement in the wrong place. This
    # runs in a subshell, so the failure is counted through a file.
    if [ "$onset" = none ]; then
        echo "FAIL no 1000 Hz onset (above -40 dBFS) in $1" | tee -a "$work/onset-failures" >&2
        onset=1000
    fi
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

# The links of the first port of the engine's monitor stream.
monitor_links() { pw-link -l | grep -A2 '^vinheta-monitor-[0-9-]*:output_FL'; }

# list_blocks LOG [KIND]: the "devices changed" blocks of a log (or the
# "targets changed" ones, with the kind "target"), one per line.
list_blocks() {
    awk -v kind="${2:-output}" -v title="${2:-device}s changed" '
         $0 == title { if (block) print block; block = "#"; next }
         index($0, kind ": ") == 1 { if (block) block = block " " $0; next }
         END { if (block) print block }' "$1"
}

# after FILE SECONDS [MARGIN]: where to measure the effect of an action that
# happens SECONDS into the playback. The tone starts tone_delay_ms into the
# sound files, and the window starts MARGIN seconds (0.7 by default) after
# the action.
tone_delay_ms=1500
after() {
    window "$1" "$(python3 -c 'import sys; print(float(sys.argv[1]) - float(sys.argv[2]) / 1000 + float(sys.argv[3]))' "$2" "$tone_delay_ms" "${3:-0.7}")"
}

# The node names of the default source and sink that no longer exist.
missing_defaults() {
    local name
    for name in $(echo "$defaults_before" | grep -o '"name": *"[^"]*"' | sed 's/.*"\([^"]*\)"$/\1/'); do
        node_exists "$name" || echo "$name"
    done
}

defaults_before=$(defaults)

ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=4" \
    -af "volume=-12dB,adelay=$tone_delay_ms:all=1,pan=stereo|c0=c0|c1=c0" -ar 48000 "$sound" || exit 1

nodes+=("$(create_node "$mic" "Vinheta test microphone" Audio/Source/Virtual MONO)")
nodes+=("$(create_node "$monitor" "Vinheta test monitor" Audio/Sink "FL FR")")
# A second microphone, silent and stereo, and a second sink.
mic2_id=$(create_node "$mic2" "Vinheta test microphone 2" Audio/Source/Virtual "FL FR")
monitor2_id=$(create_node "$monitor2" "Vinheta test monitor 2" Audio/Sink "FL FR")
nodes+=("$mic2_id" "$monitor2_id")
# A live GStreamer source feeding an unmanaged pipewiresink stalls the whole
# graph (docs/audio.md, known issues), so the fake voice is a file.
ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=600" \
    -af "volume=-12dB" -ac 1 -ar 48000 "$work/voice.wav" || exit 1
# Without state.restore-props=false WirePlumber applies the volume it saved
# for earlier pw-play and pw-record streams.
pw-play -P node.autoconnect=false -P state.restore-props=false \
    -P node.name=vinheta-test-tone "$work/voice.wav" &
pids+=($!)
wait_for "the fake microphone tone" port_exists vinheta-test-tone:output_MONO || exit 1
pw-link vinheta-test-tone:output_MONO "$mic:input_MONO"

once=(--once)

if section "both branches"; then
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
fi

if section "call volume 0"; then
    playback call-muted "${once[@]}" --call-volume 0
    start=$(window "$work/call-muted-monitor.wav")
    expect "call" "$work/call-muted-call.wav" 1000 absent "$start"
    expect "call" "$work/call-muted-call.wav" 440 present "$start"
    expect "monitor" "$work/call-muted-monitor.wav" 1000 present "$start"
fi

if section "monitor volume 0"; then
    playback monitor-muted "${once[@]}" --monitor-volume 0
    start=$(window "$work/monitor-muted-call.wav")
    expect "call" "$work/monitor-muted-call.wav" 1000 present "$start"
    expect "monitor" "$work/monitor-muted-monitor.wav" 1000 absent "$start"
fi

# 8 seconds of tone, so there is room to act in the middle of it.
long="$work/sound-long.wav"
other="$work/sound-other.wav"
for spec in "1000 $long" "2000 $other"; do
    ffmpeg -v error -y -f lavfi -i "sine=frequency=${spec%% *}:duration=8" \
        -af "volume=-12dB,adelay=$tone_delay_ms:all=1,pan=stereo|c0=c0|c1=c0" -ar 48000 "${spec#* }" || exit 1
done

# The actions below happen 4 s into the playback, in the middle of the tone.
if section "stop one sound"; then
    files=("$long") run_for=8
    playback stop --stop-after 4
    check "subject and node still there after the stop" "$subject_alive"
    for branch in call monitor; do
        expect "$branch before the stop" "$work/stop-$branch.wav" 1000 present "$(window "$work/stop-$branch.wav")" 1.5
        expect "$branch after the stop" "$work/stop-$branch.wav" 1000 absent "$(after "$work/stop-$branch.wav" 4)"
        # Without a fade the sound is cut at once.
        expect "$branch 150 ms after the stop" "$work/stop-$branch.wav" 1000 absent "$(after "$work/stop-$branch.wav" 4 0.15)" 0.5
    done
    expect "call after the stop" "$work/stop-call.wav" 440 present "$(after "$work/stop-call.wav" 4)"
fi

if section "stop all sounds"; then
    files=("$long" "$other") run_for=8
    playback stop-all --stop-all-after 4
    check "subject and node still there after the stop" "$subject_alive"
    for branch in call monitor; do
        for freq in 1000 2000; do
            expect "$branch before the stop" "$work/stop-all-$branch.wav" "$freq" present "$(window "$work/stop-all-$branch.wav")" 1.5
            expect "$branch after the stop" "$work/stop-all-$branch.wav" "$freq" absent "$(after "$work/stop-all-$branch.wav" 4)"
        done
    done
fi

if section "mute and unmute the call branch"; then
    files=("$long")
    playback mute --once --mute-call-after 4 --unmute-call-after 6.5
    call="$work/mute-call.wav"
    expect "call before the mute" "$call" 1000 present "$(window "$call")" 1.5
    expect "call while muted" "$call" 1000 absent "$(after "$call" 4 0.5)" 1.5
    expect "call while muted" "$call" 440 present "$(after "$call" 4 0.5)" 1.5
    expect "monitor while muted" "$work/mute-monitor.wav" 1000 present "$(after "$work/mute-monitor.wav" 4 0.5)" 1.5
    expect "call after the unmute" "$call" 1000 present "$(after "$call" 6.5 0.5)" 1.5
fi

if section "call branch off from the start"; then
    playback no-call --once --no-call
    start=$(window "$work/no-call-monitor.wav")
    expect "call" "$work/no-call-call.wav" 1000 absent "$start"
    expect "call" "$work/no-call-call.wav" 440 present "$start"
    expect "monitor" "$work/no-call-monitor.wav" 1000 present "$start"
fi

for branch in call monitor; do
    other=monitor
    [ "$branch" = monitor ] && other=call
    if section "$branch volume change"; then
        files=("$long")
        playback "$branch-volume" --once "--$branch-volume-after" 4 0.1
        changed="$work/$branch-volume-$branch.wav"
        kept="$work/$branch-volume-$other.wav"
        value=$(drop "$changed" 1000 "$(window "$changed")" "$(after "$changed" 4)")
        check "$branch falls by 20 dB at gain 0.1 ($value dB)" "$(between "$value" 17 23)"
        value=$(drop "$changed" 1000 "$(window "$changed")" "$(after "$changed" 4 0.2)" 0.5)
        check "$branch has fallen 200 ms after the change ($value dB)" "$(between "$value" 17 23)"
        value=$(drop "$kept" 1000 "$(window "$kept")" "$(after "$kept" 4)")
        check "$other keeps its level ($value dB)" "$(between "$value" -2 2)"
        call="$work/$branch-volume-call.wav"
        value=$(drop "$call" 440 "$(window "$call")" "$(after "$call" 4)")
        check "the voice keeps its level ($value dB)" "$(between "$value" -2 2)"
    fi
done

if section "playback volume"; then
    files=("$long")
    playback playback-volume --once --volume 0.1 --playback-volume-after 4 1.0
    # At gain 0.1 the tone is below the level that counts as its onset, so
    # the onset found is the change itself, 2.5 s after the tone started.
    for branch in call monitor; do
        file="$work/playback-volume-$branch.wav"
        value=$(drop "$file" 1000 "$(window "$file" -2)" "$(window "$file" 0.7)")
        check "$branch rises by 20 dB from gain 0.1 to 1 ($value dB)" "$(between "$value" -23 -17)"
        value=$(drop "$file" 1000 "$(window "$file" -0.7)" "$(window "$file" 0.2)" 0.5)
        check "$branch has risen 200 ms after the change ($value dB)" "$(between "$value" -23 -17)"
    done
    call="$work/playback-volume-call.wav"
    value=$(drop "$call" 440 "$(window "$call" -2)" "$(window "$call" 0.7)")
    check "the voice keeps its level ($value dB)" "$(between "$value" -2 2)"
fi

if section "playback volume and branch volume"; then
    files=("$long")
    playback playback-branch-volume --once --volume 0.5 --call-volume-after 4 0.1
    file="$work/playback-branch-volume-call.wav"
    value=$(drop "$file" 1000 "$(window "$file")" "$(after "$file" 4)")
    check "call falls by 20 dB more ($value dB)" "$(between "$value" 17 23)"
    file="$work/playback-branch-volume-monitor.wav"
    value=$(drop "$file" 1000 "$(window "$file")" "$(after "$file" 4)")
    check "monitor keeps the playback gain ($value dB)" "$(between "$value" -2 2)"
fi

# A 0 dBFS tone from 1.5 s to 5.5 s, then -20 dBFS to 8 s. The sine filter
# of ffmpeg is fixed at -18 dBFS, so the amplitude is explicit.
loud="$work/sound-loud.wav"
ffmpeg -v error -y -f lavfi \
    -i "aevalsrc=if(lt(t\,1.5)\,0\,if(lt(t\,5.5)\,1\,0.1))*sin(2*PI*1000*t):s=48000:d=8" \
    -af "pan=stereo|c0=c0|c1=c0" -ar 48000 "$loud" || exit 1

if section "call limiter"; then
    files=("$loud")
    playback limiter --once
    call="$work/limiter-call.wav"
    monitor_file="$work/limiter-monitor.wav"
    start=$(window "$call")
    value=$(analyze level "$call" 0 1000 "$start" 2)
    check "call has the loud tone at -6 dBFS ($value dBFS)" "$(between "$value" -7 -5)"
    value=$(analyze peak "$call" 0 "$start" 2)
    check "call peaks below -5 dBFS ($value dBFS)" "$(louder -5 "$value" && echo ok)"
    for freq in 2000 3000; do
        value=$(analyze level "$call" 0 "$freq" "$start" 2)
        check "call has no harmonic at $freq Hz ($value dBFS)" "$(louder -50 "$value" && echo ok)"
    done
    value=$(analyze level "$monitor_file" 0 1000 "$(window "$monitor_file")" 2)
    check "monitor has the tone at full level ($value dBFS)" "$(between "$value" -1 0.5)"
    value=$(analyze level "$call" 0 1000 "$(after "$call" 5.5)" 1.5)
    check "call has the quiet tone untouched after the release ($value dBFS)" "$(between "$value" -21 -19)"
    value=$(drop "$call" 440 "$(window "$call" -0.9)" "$start" 0.7)
    check "the voice keeps its level ($value dB)" "$(between "$value" -2 2)"

    for run in "limiter-off off" "limiter-start-off on"; do
        label=${run% *} state=${run#* }
        options=(--once --limiter-after 4 "$state")
        [ "$state" = on ] && options+=(--no-limiter)
        files=("$loud")
        playback "$label" "${options[@]}"
        call="$work/$label-call.wav"
        monitor_file="$work/$label-monitor.wav"
        value=$(drop "$call" 1000 "$(window "$call")" "$(after "$call" 4 0.2)" 0.5)
        if [ "$state" = off ]; then
            check "call rises by 6 dB 200 ms after the limiter is off ($value dB)" "$(between "$value" -7 -5)"
            value=$(analyze peak "$call" 0 "$(after "$call" 4 0.2)" 0.5)
            check "call reaches full scale without the limiter ($value dBFS)" "$(louder "$value" -0.5 && echo ok)"
        else
            check "call falls by 6 dB 200 ms after the limiter is on ($value dB)" "$(between "$value" 5 7)"
        fi
        value=$(drop "$monitor_file" 1000 "$(window "$monitor_file")" "$(after "$monitor_file" 4 0.2)" 0.5)
        check "monitor keeps its level ($value dB)" "$(between "$value" -1 1)"
    done
fi

# One second of tone with no silence around it. The call app records the
# silent microphone in the loop checks, so that silence on the call
# recording means a gap.
loop="$work/sound-loop.wav"
ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=1" \
    -af "volume=-12dB,pan=stereo|c0=c0|c1=c0" -ar 48000 "$loop" || exit 1

if section "loop"; then
    files=("$loop") run_for=7 call_mic=$mic2
    playback loop --start-after 1 --loop --stop-after 3.5
    for branch in call monitor; do
        file="$work/loop-$branch.wav"
        expect "$branch in the third pass" "$file" 1000 present "$(window "$file" 2)" 1
        value=$(analyze gaps "$file" 0 "$(window "$file" 0.2)" "$(window "$file" 3)")
        check "$branch has no gap at the seams (longest silence: $value ms)" "$(between "$value" 0 15)"
        expect "$branch after the stop" "$file" 1000 absent "$(window "$file" 4.2)" 1
    done
    check "no end is reported while it loops" "$(grep -q '^finished playing' "$work/loop.log" || echo ok)"
fi

if section "loop off"; then
    files=("$loop") call_mic=$mic2
    playback loop-off --start-after 1 --once --loop --loop-off-after 1.5
    check "subject exited with status 0" "$([ "$subject_status" -eq 0 ] && echo ok)"
    check "subject exited within 4 s of the start of the sound ($subject_seconds s with the 1 s wait)" \
        "$(between "$subject_seconds" 0 5)"
    check "the end is reported once" "$([ "$(grep -c '^finished playing' "$work/loop-off.log")" -eq 1 ] && echo ok)"
    value=$(analyze span "$work/loop-off-call.wav" 0)
    check "the tone lasts two passes ($value s)" "$(between "$value" 1.8 2.2)"
fi

if section "restart"; then
    files=("$long")
    playback restart --once --restart-after 4
    for branch in call monitor; do
        file="$work/restart-$branch.wav"
        expect "$branch before the restart" "$file" 1000 present "$(window "$file")" 1.5
        expect "$branch in the silence after the restart" "$file" 1000 absent "$(after "$file" 4 0.4)" 0.8
        expect "$branch playing again" "$file" 1000 present "$(after "$file" 4 1.9)" 1.5
    done
    expect "call after the restart" "$work/restart-call.wav" 440 present "$(after "$work/restart-call.wav" 4 0.4)" 0.8
fi

if section "position"; then
    files=("$long")
    playback position --once --position-after 2
    read -r _ elapsed duration < <(grep '^position ' "$work/position.log")
    check "elapsed time 2 s into the playback (${elapsed:-none} ms)" "$(between "${elapsed:-0}" 1500 2500 2>/dev/null)"
    check "duration of the file (${duration:-none} ms)" "$(between "${duration:-0}" 9400 9600 2>/dev/null)"
fi

if section "fade out"; then
    files=("$long") run_for=8
    playback fade --fade-out 0.3 --stop-after 4
    for branch in call monitor; do
        file="$work/fade-$branch.wav"
        value=$(drop "$file" 1000 "$(window "$file")" "$(after "$file" 4 0.1)" 0.1)
        check "$branch is fading 100 to 200 ms after the stop ($value dB down)" "$(between "$value" 1 15)"
        expect "$branch 600 ms after the stop" "$file" 1000 absent "$(after "$file" 4 0.6)" 1.5
    done
    call="$work/fade-call.wav"
    value=$(drop "$call" 440 "$(window "$call")" "$(after "$call" 4)")
    check "the voice keeps its level ($value dB)" "$(between "$value" -2 2)"
    check "no end is reported for the stopped sound" "$(grep -q '^finished playing' "$work/fade.log" || echo ok)"
fi

if section "fade and exit"; then
    "${subject[@]}" --target "$app" --monitor "$monitor" --fade-out 2 --stop-all-after 4 "$long" \
        </dev/null >"$work/fade-exit.log" 2>&1 &
    subject_pid=$!
    if wait_for "the engine" ready "$work/fade-exit.log" && sleep 4.5; then
        check "the sound is still fading" "$(streams | grep -q . && echo ok)"
        kill -INT "$subject_pid"
        check "nothing left after an exit during a fade" "$(gone_within_2s && [ -z "$(streams)" ] && echo ok)"
    else
        check "subject started for the fade and exit" fail
    fi
    wait "$subject_pid" 2>/dev/null
fi

if section "device list"; then
    "${subject[@]}" --target "$app" --monitor "$monitor" </dev/null >"$work/devices.log" 2>&1 &
    subject_pid=$!
    if wait_for "the engine" ready "$work/devices.log"; then
        sleep 0.5
        extra=$(create_node vinheta-test-extra "Vinheta test extra" Audio/Sink "FL FR")
        sleep 2
        with_extra=$(list_blocks "$work/devices.log" | tail -n 1)
        pw-cli destroy "$extra" >/dev/null
        sleep 2
    fi
    kill -INT "$subject_pid" 2>/dev/null
    wait "$subject_pid" 2>/dev/null
    first=$(list_blocks "$work/devices.log" | head -n 1)
    last=$(list_blocks "$work/devices.log" | tail -n 1)
    check "the fake sink is listed with its description" \
        "$([[ $first == *"output: $monitor (Vinheta test monitor)"* ]] && echo ok)"
    check "the drain node is never listed" \
        "$(grep -q '^output: vinheta-drain-' "$work/devices.log" || echo ok)"
    check "a sink plugged in shows up within 2 s" \
        "$([[ $with_extra == *"output: vinheta-test-extra (Vinheta test extra)"* ]] && echo ok)"
    check "a removed sink leaves the list" "$([[ $last != *vinheta-test-extra* ]] && echo ok)"
fi

if section "no app is recording"; then
    no_call_app=1
    playback no-app "${once[@]}"
    check "subject exited with status 0" "$([ "$subject_status" -eq 0 ] && echo ok)"
    expect "monitor" "$work/no-app-monitor.wav" 1000 present "$(window "$work/no-app-monitor.wav")"
fi

if section "the chosen app is not recording"; then
    playback absent-app "${once[@]}" --target vinheta-test-nobody
    check "subject exited with status 0" "$([ "$subject_status" -eq 0 ] && echo ok)"
    start=$(window "$work/absent-app-monitor.wav")
    expect "another app" "$work/absent-app-call.wav" 1000 absent "$start"
    expect "another app" "$work/absent-app-call.wav" 440 present "$start"
    expect "monitor" "$work/absent-app-monitor.wav" 1000 present "$start"
fi

if section "target switch"; then
    files=("$long") with_other_app=1
    playback target-switch --once --target-after 4 "$other_app"
    first="$work/target-switch-call.wav"
    second="$work/target-switch-other.wav"
    expect "first app before the switch" "$first" 1000 present "$(window "$first")" 1.5
    expect "first app 200 ms after the switch" "$first" 1000 absent "$(after "$first" 4 0.2)" 0.5
    expect "first app after the switch" "$first" 440 present "$(after "$first" 4)" 1.5
    # The tone reaches the second app when the target changes, 2.5 s after
    # it started.
    expect "second app before the switch" "$second" 1000 absent "$(window "$second" -2)" 1.5
    expect "second app after the switch" "$second" 1000 present "$(window "$second" 0.2)" 1.5
fi

# The app starts recording 4 s into the playback, in the middle of the tone.
if section "an app that starts recording while a sound plays"; then
    late="$work/late-app-call.wav"
    (
        sleep 4
        timeout -s INT 5 pw-record --format s16 --rate 48000 --channels 2 --target "$mic" \
            -P "{ state.restore-props=false node.name=$app application.process.binary=$app }" "$late"
    ) &
    watcher=$!
    files=("$long") no_call_app=1
    playback late-app "${once[@]}"
    wait "$watcher"
    value=$(analyze onset "$late" 0 1000)
    check "the sound reaches the app within 500 ms ($value ms)" "$(between "$value" 0 500 2>/dev/null)"
    expect "the app that came late" "$late" 1000 present 1 1.5
fi

# The second microphone is stereo, so the ports of the app are replaced
# and the links of the engine with them. It is silent, so the voice going
# away shows that the app moved.
if section "an app moved to another microphone"; then
    (
        sleep 4.5
        pw-metadata "$(node_id "$app")" target.object "$mic2" >/dev/null
    ) &
    watcher=$!
    files=("$long")
    playback app-moved "${once[@]}"
    wait "$watcher"
    call="$work/app-moved-call.wav"
    expect "call before the move" "$call" 440 present "$(window "$call")" 1.5
    expect "call after the move" "$call" 440 absent "$(after "$call" 4.5 1)" 1.5
    expect "call after the move" "$call" 1000 present "$(after "$call" 4.5 1)" 1.5
    value=$(analyze gaps "$call" 0 "$(window "$call")" "$(after "$call" 4.5 2)")
    check "the sound is back within 200 ms (longest silence: $value ms)" "$(between "$value" 0 200)"
fi

# With every app as the target the sounds would reach the real apps that
# are recording, so this one plays with the call volume at 0 and looks at
# the links instead.
if section "every recording app"; then
    extras=${#pids[@]}
    pw-record --target "$mic" \
        -P "{ state.restore-props=false stream.monitor=true node.name=vinheta-test-meter application.process.binary=vinheta-test-meter }" /dev/null &
    pids+=($!)
    record vinheta-test-desktop /dev/null
    pw-link "$monitor:monitor_FL" vinheta-test-desktop:input_FL
    pw-link "$monitor:monitor_FR" vinheta-test-desktop:input_FR
    (
        sleep 3.5
        for name in "$app" "$other_app" vinheta-test-meter vinheta-test-desktop; do
            call_linked "$name" && echo "$name"
        done >"$work/all-apps-linked.txt"
    ) &
    watcher=$!
    files=("$long") with_other_app=1 run_for=5
    playback all-apps --target all --call-volume 0
    wait "$watcher"
    kill -INT "${pids[@]:$extras}" 2>/dev/null
    wait "${pids[@]:$extras}" 2>/dev/null
    pids=("${pids[@]:0:$extras}")
    linked() { grep -qx "$1" "$work/all-apps-linked.txt"; }
    check "the sound is linked to both apps" "$(linked "$app" && linked "$other_app" && echo ok)"
    check "the sound is not linked to a level meter" "$(linked vinheta-test-meter || echo ok)"
    check "the sound is not linked to a recorder of an output" "$(linked vinheta-test-desktop || echo ok)"
    targets=$(list_blocks "$work/all-apps.log" target | grep -F "$other_app" | tail -n 1)
    check "both apps are listed with their names" \
        "$([[ $targets == *"target: $app (Vinheta test app $app)"*"target: $other_app (Vinheta test app $other_app)"* ]] && echo ok)"
    check "the meter and the recorder of an output are never listed" \
        "$(grep -Eq '^target: vinheta-test-(meter|desktop) ' "$work/all-apps.log" || echo ok)"
fi

if section "monitor switch"; then
    files=("$long") record_monitor2=1
    playback monitor-switch --once --monitor-after 4 "$monitor2"
    first="$work/monitor-switch-monitor.wav"
    second="$work/monitor-switch-monitor2.wav"
    call="$work/monitor-switch-call.wav"
    expect "first sink before the switch" "$first" 1000 present "$(window "$first")" 1.5
    expect "first sink 1 s after the switch" "$first" 1000 absent "$(after "$first" 4 1)" 1.5
    expect "second sink after the switch" "$second" 1000 present "$(window "$second")" 1.5
    # The tone reaches the second sink when the stream is moved, 4 s into the
    # playback, which is 2.5 s after it reached the first one.
    moved=$(python3 -c 'import sys; print(f"{(float(sys.argv[2]) - float(sys.argv[1])) / 1000:.2f}")' \
        "$(analyze onset "$first" 0 1000)" "$(analyze onset "$second" 0 1000)" 2>/dev/null)
    check "the stream moved within 1 s of the request ($moved s after the tone started)" "$(between "${moved:-0}" 2 3.5)"
    expect "call before the switch" "$call" 1000 present "$(window "$call")" 1.5
    expect "call during the switch" "$call" 1000 present "$(after "$call" 4 -0.3)" 1
    expect "call after the switch" "$call" 1000 present "$(after "$call" 4)" 1.5
fi

# The output changes before the tone of the first playback starts, and
# the files are played again after it ended. Without --once, which would
# exit before that.
if section "next sound uses the new output"; then
    record_monitor2=1 run_for=12
    playback next-output --monitor-after 0.5 "$monitor2" --replay-after 6.5
    first="$work/next-output-monitor.wav"
    second="$work/next-output-monitor2.wav"
    check "nothing reaches the first sink" "$([ "$(analyze onset "$first" 0 1000)" = none ] && echo ok)"
    expect "second sink, first playback" "$second" 1000 present "$(window "$second")"
    # The second playback starts at 6.5 s, so its tone starts at 8 s.
    expect "second sink, second playback" "$second" 1000 present "$(after "$second" 8 0.5)"
fi

# The fallback of a removed output is the real default sink, so this one
# plays with the monitor volume at 0.
if section "output removed while playing"; then
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
fi

# The recorders are started for the last engine only, so the recordings
# hold its playback alone.
if section "a second engine in the same process"; then
    "${subject[@]}" --target "$app" --monitor "$monitor" --once --restart-engine-after 1 "$sound" \
        </dev/null >"$work/second-engine.log" 2>&1 &
    subject_pid=$!
    ready_thrice() { [ "$(grep -c '^engine ready' "$work/second-engine.log")" -eq 3 ]; }
    if wait_for "the third engine" ready_thrice; then
        recorders=${#pids[@]}
        call_app "$app" "$work/second-engine-call.wav"
        record vinheta-test-rec-monitor "$work/second-engine-monitor.wav"
        pw-link "$monitor:monitor_FL" vinheta-test-rec-monitor:input_FL
        pw-link "$monitor:monitor_FR" vinheta-test-rec-monitor:input_FR
        count=$(pw-dump | grep -c '"node.name": "vinheta-drain-')
        check "the drain node exists once after the restarts ($count found)" "$([ "$count" -eq 1 ] && echo ok)"
        wait "$subject_pid"
        check "subject exited with status 0" "$([ $? -eq 0 ] && echo ok)"
        sleep 0.3
        kill -INT "${pids[@]:$recorders}" 2>/dev/null
        wait "${pids[@]:$recorders}" 2>/dev/null
        pids=("${pids[@]:0:$recorders}")
        call="$work/second-engine-call.wav"
        start=$(window "$call")
        expect "call of the third engine" "$call" 1000 present "$start"
        expect "call of the third engine" "$call" 440 present "$start"
        expect "monitor of the third engine" "$work/second-engine-monitor.wav" 1000 present \
            "$(window "$work/second-engine-monitor.wav")"
        threads=$(sed -n 's/^threads //p' "$work/second-engine.log" | tr '\n' ' ')
        read -r first second <<<"$threads"
        check "the threads do not grow from one start to the next ($threads)" \
            "$([ -n "${second:-}" ] && [ "$second" -le "$first" ] && echo ok)"
        check "nothing left after the exit" "$(gone_within_2s && echo ok)"
    else
        check "subject restarted its engine twice" fail
        kill -INT "$subject_pid" 2>/dev/null
        wait "$subject_pid" 2>/dev/null
    fi
fi

# A private PipeWire instance: no session manager and no devices, and killing
# it harms nothing. Nothing is played here: the monitor branch never links.
if section "a lost connection is reported"; then
    private="audio-test-pipewire-$$"
    private_socket="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$private"
    PIPEWIRE_REMOTE=$private "${subject[@]}" </dev/null >"$work/unreachable.log" 2>&1
    check "the start fails without the instance" "$([ $? -ne 0 ] && echo ok)"
    check "the kind of the error is unreachable" "$(grep -q '^error: unreachable: ' "$work/unreachable.log" && echo ok)"
    PIPEWIRE_CORE=$private pipewire >"$work/private-pipewire.log" 2>&1 &
    private_pid=$!
    pids+=("$private_pid")
    socket_ready() { [ -S "$private_socket" ]; }
    if wait_for "the private instance" socket_ready; then
        PIPEWIRE_REMOTE=$private "${subject[@]}" </dev/null >"$work/lost.log" 2>&1 &
        subject_pid=$!
        if wait_for "the engine on the private instance" ready "$work/lost.log"; then
            kill "$private_pid"
            reported=
            for _ in $(seq 20); do
                grep -q '^error: connection-lost: ' "$work/lost.log" && reported=ok && break
                sleep 0.1
            done
            check "the lost connection is reported within 2 s" "$reported"
        else
            check "subject started on the private instance" fail
        fi
        kill -INT "$subject_pid" 2>/dev/null
        wait "$subject_pid" 2>/dev/null
    else
        check "the private instance started" fail
    fi
    kill "$private_pid" 2>/dev/null
    wait "$private_pid" 2>/dev/null
    rm -f "$private_socket" "$private_socket.lock" "$private_socket-manager" "$private_socket-manager.lock"
    check "the private instance and its socket are gone" \
        "$(! kill -0 "$private_pid" 2>/dev/null && [ ! -e "$private_socket" ] && echo ok)"
fi

for signal in INT KILL; do
    if section "cleanup after SIG$signal"; then
        "${subject[@]}" --target "$app" --monitor "$monitor" "$sound" </dev/null >"$work/sig$signal.log" 2>&1 &
        subject_pid=$!
        recorders=${#pids[@]}
        if wait_for "the engine" ready "$work/sig$signal.log" && call_app "$app" /dev/null && sleep 2.5; then
            check "the sound is linked to the app before SIG$signal" "$(call_linked "$app" && echo ok)"
            kill "-$signal" "$subject_pid"
            check "nothing left after SIG$signal" "$(gone_within_2s && echo ok)"
            check "the app is still fed by its microphone" "$(fed "$app" && ! call_linked "$app" && echo ok)"
        else
            check "subject started for SIG$signal" fail
        fi
        wait "$subject_pid" 2>/dev/null
        kill -INT "${pids[@]:$recorders}" 2>/dev/null
        wait "${pids[@]:$recorders}" 2>/dev/null
        pids=("${pids[@]:0:$recorders}")
    fi
done

cleanup
sleep 0.3
stray=$(pw-dump | grep -c '"node.name": "vinheta' || true)
check "no vinheta node left ($stray found)" "$([ "$stray" -eq 0 ] && echo ok)"
# A real device unplugged during the run changes the defaults by itself. The
# checks only use fake devices, so that is reported without failing.
unplugged=$(missing_defaults)
if [ "$(defaults)" != "$defaults_before" ] && [ -n "$unplugged" ]; then
    echo "NOTE the default devices changed because a device went away during the run:" $unplugged
else
    check "default source and sink unchanged" "$([ "$(defaults)" = "$defaults_before" ] && echo ok)"
fi

[ -f "$work/onset-failures" ] && failures=$((failures + $(wc -l <"$work/onset-failures")))
if [ "$failures" -eq 0 ]; then
    echo "ALL CHECKS PASSED"
else
    echo "$failures CHECK(S) FAILED"
    exit 1
fi
