#!/usr/bin/env bash
# Prints the version when meson.build, Cargo.toml, debian/changelog, and the
# metainfo agree, and fails naming the ones that differ. With a tag
# (version.sh v1.0.0) it also fails when the tag is not that version.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

meson=$(sed -n "s/^ *version: '\([^']*\)'.*/\1/p" meson.build | head -n 1)
cargo=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
lock=$(grep -A1 '^name = "vinheta"$' Cargo.lock | sed -n 's/^version = "\([^"]*\)"/\1/p')
debian=$(sed -n '1s/^vinheta (\([^)-]*\)-[^)]*).*/\1/p' debian/changelog)
metainfo=$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' data/io.github.wilfison.Vinheta.metainfo.xml.in | head -n 1)

status=0
for place in "Cargo.toml $cargo" "Cargo.lock $lock" "debian/changelog $debian" "metainfo $metainfo"; do
    if [ "${place#* }" != "$meson" ]; then
        echo "${place%% *} has version '${place#* }', meson.build has '$meson'" >&2
        status=1
    fi
done
if [ $# -gt 0 ] && [ "$1" != "v$meson" ]; then
    echo "the tag $1 does not match the version $meson" >&2
    status=1
fi
[ "$status" -eq 0 ] && echo "$meson"
exit "$status"
