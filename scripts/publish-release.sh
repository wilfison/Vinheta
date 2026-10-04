#!/usr/bin/env bash
# Creates the GitHub release of a tag from the package built in tmp/deb, or
# updates it when it exists (a second run for the same tag replaces the files
# and the notes, and never makes a second release). Run by the release
# workflow; it needs gh with a token that can write releases.
set -euo pipefail

[ $# -eq 1 ] || { echo "usage: publish-release.sh TAG" >&2; exit 2; }
tag=$1
cd "$(dirname "$0")/.."

scripts/version.sh "$tag" >/dev/null
scripts/release-notes.sh "$tag" >tmp/deb/release-notes.md
deb=$(ls tmp/deb/vinheta_"${tag#v}"-*_amd64.deb | head -n 1)

if gh release view "$tag" >/dev/null 2>&1; then
    gh release upload "$tag" "$deb" "$deb.sha256" --clobber
    gh release edit "$tag" --notes-file tmp/deb/release-notes.md
else
    gh release create "$tag" "$deb" "$deb.sha256" --title "$tag" --notes-file tmp/deb/release-notes.md --latest
fi
