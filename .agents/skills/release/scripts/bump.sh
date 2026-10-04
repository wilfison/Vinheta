#!/usr/bin/env bash
# Bumps the version of Vinheta in its four places: meson.build, Cargo.toml
# (and Cargo.lock), a new entry of debian/changelog, and a new <release> of
# the metainfo, the last two with the same date and the summary given.
# Without --write it only prints the numbers.
set -euo pipefail

usage() {
    echo "usage: bump.sh {major|minor|patch} [--write --summary TEXT]" >&2
    exit 2
}

bump=${1:-}
case $bump in major | minor | patch) shift ;; *) usage ;; esac
write=
summary=
while [ $# -gt 0 ]; do
    case $1 in
        --write) write=1 ;;
        --summary) shift; summary=${1:-} ;;
        *) usage ;;
    esac
    shift
done

cd "$(dirname "$0")/../../../.."
metainfo=data/io.github.wilfison.Vinheta.metainfo.xml.in

# Fails when the four places differ: that is fixed by hand, not bumped over.
old=$(scripts/version.sh)
[[ $old =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || { echo "the version '$old' is not MAJOR.MINOR.PATCH" >&2; exit 1; }
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}
case $bump in
    major) new="$((major + 1)).0.0" ;;
    minor) new="$major.$((minor + 1)).0" ;;
    patch) new="$major.$minor.$((patch + 1))" ;;
esac

echo "version: $old -> $new"
echo "tag:     v$new"
if [ -z "$write" ]; then
    echo "(dry run: run again with --write --summary TEXT to apply)"
    exit 0
fi
[ -n "$summary" ] || { echo "--write needs --summary TEXT (one sentence for debian/changelog and the metainfo)" >&2; exit 2; }
if git rev-parse -q --verify "refs/tags/v$new" >/dev/null; then
    echo "the tag v$new already exists" >&2
    exit 1
fi

old_re=${old//./\\.}
sed -i "0,/^\( *version: '\)$old_re'/s//\1$new'/" meson.build
sed -i "0,/^version = \"$old_re\"/s//version = \"$new\"/" Cargo.toml
sed -i "/^name = \"vinheta\"\$/{n;s/^version = \"$old_re\"/version = \"$new\"/}" Cargo.lock

# The distribution and the maintainer are those of the entry before.
distribution=$(sed -n '1s/^[^)]*) \([^;]*\);.*/\1/p' debian/changelog)
maintainer=$(sed -n 's/^ -- \(.*>\)  .*/\1/p' debian/changelog | head -n 1)
{
    echo "vinheta ($new-1) $distribution; urgency=medium"
    echo
    echo "$summary" | fold -s -w 72 | sed -e 's/ *$//' -e '1s/^/  * /' -e '2,$s/^/    /'
    echo
    echo " -- $maintainer  $(LC_ALL=C date -R)"
    echo
    cat debian/changelog
} >debian/changelog.new
mv debian/changelog.new debian/changelog

escaped=$(printf '%s' "$summary" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
awk -v version="$new" -v date="$(date +%F)" -v text="$escaped" '
    { print }
    /<releases>/ && !done {
        print "    <release version=\"" version "\" date=\"" date "\">"
        print "      <description>"
        print "        <p>" text "</p>"
        print "      </description>"
        print "    </release>"
        done = 1
    }
' "$metainfo" >"$metainfo.new"
mv "$metainfo.new" "$metainfo"

scripts/version.sh >/dev/null
echo "written: meson.build, Cargo.toml, Cargo.lock, debian/changelog, $metainfo"
