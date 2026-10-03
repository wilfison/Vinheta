# Helpers shared by audio-poc.sh and verify-audio-poc.sh. Meant to be sourced.

# Prints the id of the node with the given node.name, or nothing.
node_id() {
    pw-dump | python3 -c '
import json, sys
for obj in json.load(sys.stdin):
    props = (obj.get("info") or {}).get("props") or {}
    if obj.get("type") == "PipeWire:Interface:Node" and props.get("node.name") == sys.argv[1]:
        print(obj["id"])
        break
' "$1"
}

# create_node NAME DESCRIPTION MEDIA_CLASS POSITIONS, prints the node id.
# pw-cli exits right away, so the node needs object.linger and an explicit destroy.
create_node() {
    pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=$1 node.description=\"$2\" media.class=$3 audio.position=[$4] object.linger=true }" >/dev/null
    wait_for "node $1" node_exists "$1"
    node_id "$1"
}

node_exists() { [ -n "$(node_id "$1")" ]; }
port_exists() { pw-link -io | grep -qxF "$1"; }

# wait_for LABEL COMMAND..., polls for up to 5 seconds.
wait_for() {
    local label=$1 i
    shift
    for i in $(seq 50); do
        "$@" && return 0
        sleep 0.1
    done
    echo "timed out waiting for $label" >&2
    return 1
}

# link_to_stereo SOURCE_NODE DEST_NODE PORT_PREFIX DEST_PREFIX
# A mono source feeds both channels, a stereo one is linked channel by channel.
link_to_stereo() {
    local src=$1 dst=$2 src_prefix=$3 dst_prefix=$4 ports
    ports=$(pw-link -o | grep -F "$src:${src_prefix}_" || true)
    if [ "$(printf '%s\n' "$ports" | grep -c .)" -eq 1 ]; then
        pw-link "$ports" "$dst:${dst_prefix}_FL"
        pw-link "$ports" "$dst:${dst_prefix}_FR"
    else
        pw-link "$src:${src_prefix}_FL" "$dst:${dst_prefix}_FL"
        pw-link "$src:${src_prefix}_FR" "$dst:${dst_prefix}_FR"
    fi
}

default_source() {
    pw-metadata 0 default.audio.source | sed -n 's/.*"name":"\([^"]*\)".*/\1/p'
}
