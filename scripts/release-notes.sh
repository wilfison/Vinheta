#!/usr/bin/env bash
# Prints the body of the GitHub release of a tag: the section of CHANGELOG.md,
# how to install, the checksum of the package built in tmp/deb (written next
# to it as .sha256), and the link to the changes since the tag before.
set -euo pipefail

[ $# -eq 1 ] || { echo "usage: release-notes.sh TAG" >&2; exit 2; }
tag=$1
version=${tag#v}
cd "$(dirname "$0")/.."
repository=${GITHUB_REPOSITORY:-wilfison/Vinheta}

deb=$(ls tmp/deb/vinheta_"$version"-*_amd64.deb 2>/dev/null | head -n 1)
[ -n "$deb" ] || { echo "no package of version $version in tmp/deb: run scripts/build-deb.sh" >&2; exit 1; }
name=$(basename "$deb")
(cd tmp/deb && sha256sum "$name" >"$name.sha256")

# The lines between "## [VERSION]" and the next section or the link list.
section=$(awk -v header="## [$version]" '
    index($0, header) == 1 { found = 1; next }
    found && (index($0, "## [") == 1 || /^\[[^]]+\]: /) { exit }
    found { print }
' CHANGELOG.md)
# -F: the dots of a version are not wildcards; -x: the whole line.
previous=$(git tag --sort=-v:refname | grep -Fxv "$tag" | head -n 1 || true)

if [ -n "$section" ]; then
    echo "## What's new"
    echo "$section"
    echo
fi
cat <<BODY
## Install

Vinheta is packaged for Ubuntu 26.04. Download \`$name\` below, then:

\`\`\`sh
sudo apt install ./$name
\`\`\`

## Checksum (SHA256)

\`\`\`
$(cat "$deb.sha256")
\`\`\`
BODY
if [ -n "$previous" ]; then
    echo
    echo "**Full diff**: https://github.com/$repository/compare/$previous...$tag"
fi
