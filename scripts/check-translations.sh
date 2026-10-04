#!/usr/bin/env bash
# Checks the translations without touching the working tree: po/POTFILES.in
# lists every file with strings, every gettext call of the Rust files reached
# the template, and every language of po/LINGUAS translates every string.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
require_tools xgettext msgmerge msgfmt
cd "$root" || exit 1

work=tmp/check/translations
mkdir -p "$work"
pot="$work/vinheta.pot"
failures=0
fail() {
    echo "FAIL $*"
    failures=$((failures + 1))
}

listed() { grep -v '^#' po/POTFILES.in | grep .; }

listed | LC_ALL=C sort -c 2>/dev/null || fail "po/POTFILES.in is not sorted"
for file in $(grep -l 'translatable="yes"' src/ui/*.ui) $(grep -lE '\bn?gettext\(' src/*.rs src/ui/*.rs); do
    listed | grep -qxF "$file" || fail "$file has strings and is not in po/POTFILES.in"
done

# xgettext does not know Rust and reads those files as C, with warnings. That
# has found every call so far, which the count below verifies.
xgettext --package-name=vinheta --from-code=UTF-8 --add-comments \
    --keyword=_ --keyword=N_ --keyword=C_:1c,2 --keyword=NC_:1c,2 \
    -f po/POTFILES.in -D . -o "$pot" 2>"$work/xgettext.log" || fail "xgettext failed, see $work/xgettext.log"

for file in $(listed | grep '\.rs$'); do
    calls=$(grep -v '^use ' "$file" | grep -oE '\bn?gettext\(' | wc -l)
    found=$(grep '^#:' "$pot" | grep -oE "(^| )$file:[0-9]+" | wc -l)
    [ "$calls" -eq "$found" ] || fail "$file has $calls gettext calls, the template has $found"
done

for language in $(grep -v '^#' po/LINGUAS); do
    po="po/$language.po"
    merged="$work/$language.po"
    msgmerge --quiet --no-fuzzy-matching "$po" "$pot" -o "$merged" || { fail "msgmerge of $po"; continue; }
    stats=$(LC_ALL=C msgfmt --check --statistics -o /dev/null "$merged" 2>&1) || fail "msgfmt rejects $po: $stats"
    case $stats in
        *fuzzy* | *untranslated*) fail "$po is incomplete: $stats" ;;
        *) echo "$po: $stats" ;;
    esac
    # A string of the file that is no longer in the template.
    obsolete=$(grep -c '^#~ msgid' "$merged")
    [ "$obsolete" -eq 0 ] || fail "$po has $obsolete obsolete strings: update it with msgmerge"
done

[ "$failures" -eq 0 ]
