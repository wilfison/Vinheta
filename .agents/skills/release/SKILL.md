---
name: release
description: >-
  Cut a new release of Vinheta: bump the version in its four places, write the
  CHANGELOG entry, translate the release note, run the checks, commit, tag, and
  push. Use this whenever the user asks to "release", "cut a release", "ship a
  new version", "publish", "bump the version", "tag a release", "do a 1.x
  release", "lançar uma versão", "fazer o release", "publicar", or otherwise
  prepare a versioned release of the app, even if they don't name every step.
  The skill always asks first whether it's a major, minor, or patch bump, then
  drives the whole release end to end and confirms before pushing anything to
  the remote.
---

# Release a new Vinheta version

A release is a tag. Pushing `vVERSION` triggers `.github/workflows/release.yml`,
which checks the tag against the version, runs the CI gate again, builds the
`.deb`, and creates the GitHub release with the package, its `.sha256`, and
notes made by `scripts/release-notes.sh` from `CHANGELOG.md`. The `.deb` of a
GitHub release is the only distribution channel (there is no PPA). Read
"Releasing" of `docs/packaging.md` first: this skill follows it.

The version lives in **four places** (`meson.build`, `Cargo.toml` and
`Cargo.lock`, `debian/changelog`, the `release` of the metainfo), and
`scripts/version.sh` fails when they differ. The release also needs a
`## [VERSION]` section in `CHANGELOG.md` and the translation of the release
note of the metainfo in every `.po` of `po/LINGUAS`.

The pushes at the end are the only hard-to-undo steps; everything before them is
local and safe to redo. Treat the work before the push as freely revisable, and
stop for an explicit OK at the push gate.

## Step 0: Preflight (never release a broken or dirty tree)

Stop if any of these fails; a release snapshots a known-good state:

1. **Right branch, clean tree, synced with the remote.** Confirm you're on
   `main` with `git status`. If there are uncommitted changes, surface them and
   ask whether to commit, stash, or abort; don't bundle stray edits into the
   release commit. Run `git fetch` and make sure `main` isn't behind
   `origin/main`. If there is no `origin` remote, say so and ask: without it
   nothing can be published.
2. **The quick checks pass.** `scripts/check.sh` fails early on what the long
   run of Step 5 would only find after ten minutes.
3. **Is the current version already released?** Compare `scripts/version.sh`
   with `git tag --list 'v*'`. When `v<current version>` has no tag (the first
   release, or a bump that was never tagged), ask whether to release the current
   version as it is: then skip Steps 1 and 2, only review the existing
   `CHANGELOG.md` section, and continue from Step 4.

## Step 1: Ask the bump type (always, even if the user hinted one)

Ask the user whether this is a **major**, **minor**, or **patch** release, and
show what each does from the *current* version (`scripts/version.sh`). For
example, from `1.0.1`:

- **patch** → `1.0.2`: bug fixes only, no new features or behavior changes
- **minor** → `1.1.0`: new features, backwards-compatible
- **major** → `2.0.0`: breaking changes (e.g. a pad file or a settings schema
  that older versions cannot read, a removed feature users relied on)

If the user already said e.g. "ship a patch", confirm the resulting number
rather than re-asking from scratch. Recommend a level based on what's actually
in the unreleased commits (see Step 3) if they're unsure; that's the honest
signal.

## Step 2: Bump the version

The bundled helper computes the new version and writes it to the four places.
`debian/changelog` and the metainfo each get a new entry with today's date and
the same one-sentence summary:

```bash
.agents/skills/release/scripts/bump.sh <major|minor|patch>                           # dry run, review the numbers
.agents/skills/release/scripts/bump.sh <major|minor|patch> --write --summary "TEXT"  # apply
```

`TEXT` is one user-facing sentence in English about the release, in the voice of
the entry before it in the metainfo (for example "Pads open in the audio editor,
and the tab actions moved to a context menu."). Draft it together with Step 3,
and show it to the user with the CHANGELOG entry. Do **not** hand-edit the
version numbers; the script keeps the four places and the two dates in step.
Never write the em dash character in the summary or anywhere else.

## Step 3: Draft the CHANGELOG entry

The changelog follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Draft the new section from the commits since the last release, then let the user
edit it; the commit log is the raw material, not the final prose.

1. Find the range and read the commits:
   ```bash
   git describe --tags --abbrev=0          # last release tag, e.g. v1.0.0
   git log <last-tag>..HEAD --pretty='%s%n%b'
   ```
2. Group them into Keep-a-Changelog sections, ordered **Added, Changed,
   Deprecated, Removed, Fixed, Security** (include only the non-empty ones).
   Commits here are plain sentences with no prefix, so judge each by what it
   does: a new capability is **Added**, a change to existing behavior is
   **Changed**, a bug fix is **Fixed**. Omit what users cannot see (refactors,
   tests, scripts, documentation, CI).
3. Rewrite each line as a user-facing sentence (what changed for *them*), not
   the raw commit subject. Match the voice of the existing entries: descriptive,
   one bullet per feature, no line wrapping inside a bullet.
4. Insert the new section into `CHANGELOG.md` **directly below the preamble and
   above the previous version**, dated with today's real date (`date +%F`, the
   same date `bump.sh` used). The heading must be exactly
   `## [<version>] - <YYYY-MM-DD>`: `scripts/release-notes.sh` copies everything
   under it into the GitHub release body. Also add the link of the version to
   the list at the end of the file, above the one before it:
   ```
   [<version>]: https://github.com/wilfison/Vinheta/releases/tag/v<version>
   ```
   When the file already has an `## [Unreleased]` section, start from its
   entries (merged with what the commits add), rename that heading to the
   version, and replace the `[Unreleased]: .../compare/...` link with the link
   of the version.
5. **Show the drafted entry and the summary of Step 2 to the user and ask them
   to confirm or edit both** before moving on. This is the one part that needs
   human judgment.

## Step 4: Translate the release note

The `<p>` of the new `<release>` in the metainfo is a translatable string, and
every `.po` of `po/LINGUAS` must translate every string. Follow
`docs/translations.md`:

```bash
scripts/check-translations.sh --update
# translate the new entry of every po/*.po, then:
scripts/check-translations.sh
```

## Step 5: Run the checks on the release tree

The CI runs neither the audio harness nor the app check, so the full run happens
here, on the exact tree that will be tagged:

```bash
scripts/check.sh --all      # about 6 minutes: run it in the background
```

Read its output (or `tmp/check/`) when it ends. When the packaging changed since
the last release (`git diff <last-tag>..HEAD --stat -- debian scripts/ci.sh
scripts/build-deb.sh`), also run `scripts/ci-container.sh`. Never run
`scripts/ci.sh` here: it installs packages. If anything fails, report the
failure and stop. Don't tag over red.

## Step 6: Commit

Stage only the release files and commit in the style of the repository (a plain
sentence, no prefix, no `Co-Authored-By` or any tool attribution):

```bash
git add meson.build Cargo.toml Cargo.lock debian/changelog \
    data/io.github.wilfison.Vinheta.metainfo.xml.in po/*.po CHANGELOG.md
git commit -m "Release <version>"
```

Don't include unrelated files.

## Step 7: Annotated tag

Tags are **annotated** and named `v<version>`:

```bash
scripts/version.sh v<version>                 # fails when the tag is not the version
git tag -a v<version> -m "Release <version>"
```

Use `-a` (annotated), not a lightweight tag; annotated tags carry the author,
date, and message that `git describe` expects.

## Step 8: Confirm, then push (the only irreversible step)

Show the user a summary before pushing:

- the version: old → new
- the CHANGELOG section that will ship
- the commit subject and the tag name
- the result of the checks of Step 5
- the exact commands you're about to run

Then **wait for an explicit go-ahead**. Once they confirm, push `main` first and
let the CI pass before the tag goes out:

```bash
git push origin main
gh run watch "$(gh run list --workflow=ci.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
git push origin v<version>
```

If the CI fails, stop before pushing the tag: fix it on `main`, move the local
tag to the fixed commit (`git tag -d`, then Step 7 again), and push again.

A pushed tag is hard to retract cleanly, which is why this gate exists. If the
user wants to back out *before* this step, it's all local: `git tag -d
v<version>` and `git reset --hard HEAD~1` drop the release commit (the tree was
clean before it); say so if they hesitate.

## Step 9: Confirm the GitHub release

The tag push starts the `Release` workflow, which creates the GitHub release
with the notes, the `.deb`, and its SHA256. Don't run `gh release create` or
`scripts/publish-release.sh` by hand; the workflow owns the release. Check that
it finished:

```bash
gh run list --workflow=release.yml --limit 1
gh release view v<version>
```

If the run failed, report the failing step. After a fix lands on `main`, the tag
has to move to the fixed commit before the workflow is run again (`gh workflow
run release.yml -f ref=v<version>`, which updates the release instead of making
a second one); ask the user before moving a pushed tag.

## Quick reference: the whole flow

```
preflight (main, clean, synced, scripts/check.sh, is the current version tagged?)
  → ask major/minor/patch
  → bump.sh --write --summary   (meson.build, Cargo.toml, Cargo.lock, debian/changelog, metainfo)
  → draft CHANGELOG + link, user confirms it and the summary
  → translate the release note in every po/*.po
  → scripts/check.sh --all      (background; ci-container.sh when the packaging changed)
  → git add <release files> && commit "Release X.Y.Z"
  → git tag -a vX.Y.Z -m "Release X.Y.Z"
  → SHOW SUMMARY, wait for OK
  → git push origin main, wait for the CI, git push origin vX.Y.Z
  → check the Release workflow run + gh release view vX.Y.Z
```
