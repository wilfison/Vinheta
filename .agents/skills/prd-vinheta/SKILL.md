---
name: prd-vinheta
description: "Writes the PRD of a phase or feature of Vinheta (a GTK4/libadwaita soundboard in Rust), with acceptance criteria that the project's scripts can verify. Use it in this repository instead of the generic prd skill. Triggers: create a prd, prd for phase N, plan this feature, spec out, write prd for, plan this phase, criar um prd, prd da fase N, planejar essa feature, especificar."
allowed-tools: Glob, Grep, Read, Bash, Write, Edit, AskUserQuestion
---

# Vinheta PRD generator

Writes the PRD of a phase of work or of a standalone feature, ready to be implemented.
**Do NOT implement anything.** Only write the PRD to `tasks/prd-phase-N-[name].md` (kebab-case, Markdown).
For a standalone feature, use `tasks/prd-[feature-name].md`.

Project rules that apply to the PRD:

- The PRD is written in **English**, like the rest of the repository's documentation. The conversation with the user stays in the user's language.
- Never use the em dash character (U+2014), not even inside command examples. Use a comma, colon, parentheses, or a separate sentence.
- `tasks/` is git-ignored: the PRD is never committed. It is the live progress tracker of the phase.

## Step 1: Context

Before asking anything, read:

- `AGENTS.md`: the rules and the index of the documentation.
- `docs/overview.md` (what the app does and its open items), `docs/architecture.md`, `docs/checks.md` (the verification scripts), and the document of each area the phase touches.
- The most recent PRD in `tasks/`, to keep the same structure, tone, and level of detail.
- The mockups in `mockups/` that the phase mentions (they are SVG and can be read as text), and `docs/audio.md` when the phase touches the engine.
- The code the phase will touch (`src/audio/mod.rs` for the engine API, `src/ui/` for the interface, the schema in `data/`).

## Step 2: Clarifying questions

Ask only what is critical and ambiguous, with `AskUserQuestion`, never as plain text. Up to 4 questions
per call, in batches ordered by dependency; send the next batch only after the previous one is answered.
Each question has a short `header`, 2 to 4 concrete options with the trade-off in the `description`, the
recommended one first with "(Recommended)", and `multiSelect: true` when the choices are not exclusive.

Ambiguities that usually come up in this project:

- **Mockup bigger than the phase:** the mockups show the final product. Ask what to do with the controls of later phases (omit, disable, bring forward).
- **Item that depends on later work:** confirm whether it is brought forward or postponed.
- **libadwaita widget:** when more than one is reasonable, ask, stating the minimum version each one needs.
- **Audio behavior with sounds already playing:** whether a new option applies immediately or only to the next sounds.
- **Errors:** how much of the handling goes in now and how much is left for later.

Do not ask about what has an obvious default (the PRD language, the file name, how things are verified: they are defined here).

## Step 3: Validate before writing

Every technical claim in the PRD must have been checked, not remembered. Use `Bash` to:

- Confirm that the API exists in the version in use: the crate feature in `Cargo.toml` and in the crate source under `~/.cargo/registry` or `build/cargo-home`, the symbol in the installed library (`pkg-config --modversion`, `strings`), the GStreamer element (`gst-inspect-1.0 --exists`) and the package that ships it (`dpkg -S`).
- Check that the minimum version in `debian/control` covers what the phase uses.
- Run `scripts/check.sh --all` (several minutes; run it in the background while writing) and record the starting state under "Technical Considerations". If something already fails before the phase, the first story fixes it.
- Check each interface story against the states the app already has (audio unavailable, an empty folder, a missing folder): a rule of an earlier phase, such as pads being disabled without audio, can make a new criterion impossible.
- Test cheap hypotheses on the spot (an environment variable, an action over D-Bus, a screenshot of the current app) instead of writing them down as facts.
- For a PipeWire or WirePlumber behavior, try it with the command line tools before designing on top of it: a silent file played with `gst-launch-1.0 ... ! pipewiresink`, a fake device from `create_node` in `scripts/audio-common.sh`, then `pw-link -l`, `pw-metadata`, and `pw-cli destroy`. Name the test nodes without the `vinheta` prefix, and destroy them. `docs/audio.md` has the findings so far; read it first.
- A claim about a crate API that the design depends on is worth a throwaway program in the scratchpad when reading the source is not enough.

- A claim about how the desktop sees the app (a portal, the app ID, a D-Bus service of the session, autostart) is tested the way a user runs it, never from the terminal of the agent alone: start the test with `systemd-run --user --scope`, because a terminal started by the desktop has an app scope that a portal takes for the app. Test with the files the app really installs (its own desktop file, from the local prefix and as the package installs it), not with a probe made for the test: once a probe with a valid `Exec` passed while the desktop file of the local prefix (`Exec=vinheta`, not in `PATH`) was refused, and three stories were built on it.
- Do not cut the output of a monitor (`dbus-monitor`, `pw-mon`) with `head` before looking for an error reply: filter by the sender of the app instead.
- When the core of a feature cannot be scripted (a real key press on Wayland, a system dialog, a real call), its manual check is the first story of the PRD, done by the user before anything is built on it. A measurement by proxy does not replace it.

What could not be verified goes into the PRD as a target or an assumption, with a criterion in the story that measures it. Never as a fact.

## Step 4: PRD structure

The reader may be a junior developer or an AI agent: be explicit, with numbered items.
Sections:

1. **Introduction**: the phase, the problem it solves, and a short glossary of the PipeWire, GTK, or GStreamer terms used.
2. **Goals**: specific and measurable.
3. **User Stories**: see below.
4. **Functional Requirements**: numbered (`FR-1: ...`), grouped by area (Engine, Library, Interface, Application, Verification).
5. **Non-Goals**: what is left out, saying where each item is left ("Open items" of `docs/overview.md`, or nowhere).
6. **Design Considerations**: reference mockups, what differs from them in this phase, widgets to reuse, accessibility.
7. **Technical Considerations**: architecture decisions with their reasons, the verified starting state (with the date), the limits of the tools.
8. **Success Metrics**
9. **Open Questions**: what is left, plus the list of decisions taken in Step 2.

### User Stories

Each story fits in one focused session. Order them by execution order; a story may only depend on one
numbered before it. The order that works in this project:

1. Audio engine (`src/audio/`), proven by the harness before the interface depends on it.
2. Foundations without interface (testable logic in `src/library.rs` or a new module, GSettings keys, tooling).
3. Interface (`src/ui/`), one story per screen or behavior.
4. A closing story (see below).

```markdown
### US-001: [Short descriptive title]

**Description:** As a [user or developer], I want [feature] so that [benefit].

**Depends on:** US-00X (omit the line when there is none)

**Acceptance Criteria:**

- ⬜ Specific, verifiable criterion
- ⬜ `scripts/check.sh` passes
```

State markers: `- ⬜` pending, `- ✅` done, `- 🟨` partial, `- ❌` cancelled. For partial and cancelled,
note the gap or the reason on the same line. Never use `- [ ]` / `- [x]`.
**After implementing a story**, edit the PRD and set each criterion to its final state.

### Verification criterion by kind of story

There is no browser and no Playwright here. Every story ends with the criterion of its kind:

| The story touches | Required criterion |
|---|---|
| Any code | `scripts/check.sh` passes (clippy without warnings, a single glib version, no em dash, one version, complete translations, `meson test`) |
| A new or changed UI string | `po/pt_BR.po` translates it (`scripts/check-translations.sh`), and a screenshot check with `--lang pt_BR` shows it |
| Engine (`src/audio/`) | `scripts/check.sh --audio` passes, and the harness `scripts/verify-audio.sh` gains a check for each new engine operation, exposed through an option of `vinheta-audio-test` |
| Logic without interface | Rust unit tests in the module itself, run by `cargo test --lib` |
| Interface (`src/ui/`) | "Screenshot check": `scripts/screenshot.sh NAME ...` in the described state, read and compared with the mockup and with the criteria; in the dark and `--light` styles when the story creates a new screen |
| Interface wired to the engine | `scripts/verify-app.sh` passes, with a new section (self-contained, runnable alone with `--only`) when the story changes what reaches the virtual microphone |
| Packaging or dependencies | `scripts/build-deb.sh` succeeds, `debian/control` declares what changed, and `scripts/ci-container.sh` (the CI gate, `scripts/check.sh --deb`, in a clean Ubuntu 26.04 container, with `lintian` and an install in a second container) passes |

When writing a "Screenshot check", say how the state is reached. The tools of `scripts/screenshot.sh`:

- `--folder DIR` fills the library; `--setting 'KEY VALUE'` sets any other GSettings key before the app starts; `--pads FILE` gives the app a `pads.json` (the pad settings) to start with; `--action` activates `app.*` and `win.*` actions; `--click X,Y`, `--right-click X,Y`, `--key KEYS`, `--size W,H`, `--wait SECONDS`, `--restart` (quits and starts the app again, to check what is restored), and `--exec COMMAND` run in the given order; `--no-audio` takes the engine down; `--light` switches the style; `--lang pt_BR` runs the app in that language; `--first-run` lets the call setup guide open (every other run starts with `call-guide-shown true`).
- `--private-pipewire` starts a PipeWire instance of the script's own, with no session manager and no devices; `--stop-pipewire` and `--start-pipewire` are steps. Use it for a lost connection (`--stop-pipewire`) and for a system with no microphone. Nothing plays there (a pad stays at 00:00), and `--expect-playing` does not work with it.
- A node named `vinheta` created with `--fake-sink 'vinheta Other'` makes the engine fail with the name taken; `--unplug vinheta` before `--action retry-audio` frees it.
- `--expect-setting 'KEY VALUE'` and `--expect-playing N` check a value instead of showing it, and fail the run. Write a criterion on a value with them ("`--key q --expect-playing 1`"), and keep the pictures for what only a picture shows.
- `--capture NAME` takes a picture in the middle of the sequence and `--crop WxH+X+Y` crops the ones that follow, so one run can check several states; `--sheet` stacks them in one picture.
- `scripts/fixtures.sh` creates the sound folders and pad settings for these checks in `tmp/fixtures`. Name its paths in the criteria instead of describing a fixture to build. Besides `Fixture`, `Palette`, and `Loop` it has `Many` (60 pads, enough to scroll), `Effects` (a second folder for the search), `favorites.json` (two favorites and a custom name), `shortcuts.json` (pad keys on three pads of two folders), `Broken` (a file that cannot be played), and `corrupt-pads.json` (a pad file that cannot be read).
- A check that creates, renames, or deletes files works on a copy of a fixture made by the check itself (for example `cp -r tmp/fixtures/Fixture tmp/monitor-test/Fixture`), never in `tmp/fixtures`, and removes it at the end.
- The virtual display has no window manager: the content of the window is 10 pixels narrower than the size given to `--size`, and a dialog needs a content of exactly 360 pixels. Narrow checks use `--size 370,700`.
- `--fake-mic 'NODE DESCRIPTION'` and `--fake-sink 'NODE DESCRIPTION'` create a fake device before the app starts; the steps `--plug-mic`, `--plug-sink`, and `--unplug NODE` do it while the app runs. Use them whenever a criterion names a device: the real device list differs between machines.
- `--key` reaches the app with no click before it, text fields included; an `--exec` step with `xdotool keydown` and `keyup` holds a key.
- The virtual display is 1100 by 800: a dialog taller than that is scrolled with an `--exec` step (`xdotool mousemove X Y click --repeat 12 5`).
- Dialogs opened by an action and open popovers (menus, drop-downs) are captured.
- A state stored in GSettings is reached with `--setting`. A new state that can only be reached by a click that is hard to place deserves an action (`app.*` or `win.*`) in the story itself; actions also serve the shortcuts.

Limits the PRD must respect (do not write criteria that depend on them):

- Portal dialogs (the file or folder chooser) do not work on the virtual display. Whatever depends on them is marked for manual testing.
- The app on the virtual display uses the real PipeWire: it creates the real "Vinheta" node and plays on the default output. Test sounds are silent (`silent_sound`) or the quiet tone of `tone_sound`, both in `scripts/dev-common.sh`.
- If another Vinheta instance is open, the engine fails with "node already exists". For the same reason, two checks that start the app (screenshots, `verify-app.sh`) cannot run at the same time.
- The fallback of a removed or missing device is the real default device. A check of that fallback must play nothing audible (monitor volume at 0, or a silent file) and record nothing from the real microphone.
- A level measured on the virtual microphone includes the real microphone, which can be noisy enough to hide a quiet tone. Turn "Include My Voice" off (`include-my-voice false`) before measuring.
- Real clicks on a list entry, drags, and typing are not simulated reliably. Write the criterion on the state (the setting, the action) and mark the gesture for manual testing.
- Screenshots go to `tmp/screenshots/` and are deleted at the end of the story.
- The CI (GitHub Actions, `scripts/ci.sh`) only runs `scripts/check.sh --deb`: it has no PipeWire session and no display, so the audio harness and `scripts/verify-app.sh` stay local.
- The audio harness only uses fake devices; nothing in it needs a person listening. A test on a real call is always optional.
- A harness criterion with a new sound or a new level is rehearsed before its numbers are written: the recorders are ready 0.2 to 0.3 seconds after the subject starts (a sound with no silence at its start needs `--start-after 1`), and a tone below -40 dBFS in the recording has no onset for `window` and `after` (a gain of 0.1 puts the test tone at -50).
- A screenshot criterion that names times must count the half second the script waits after each step and the time a capture takes.

### Closing story

The last story of every phase updates the documents and closes the verification:

- `AGENTS.md`: any rule that is no longer true.
- `docs/`: the document of each area that changed, "What the app does" and "Open items" of `docs/overview.md`, and "Tested by hand only" of `docs/checks.md`.
- `po/POTFILES.in` (every `.ui` and `.rs` file with translatable strings, in alphabetical order) and `src/vinheta.gresource.xml` (every new `.ui` file, with an `alias` that drops the `ui/` directory).
- `scripts/check.sh --all` passes.
- `tmp/screenshots/` is empty.

## Step 5: Final review

Before handing it over:

- Reread the PRD looking for unverified claims and vague criteria ("works correctly" is bad; "the button is insensitive while nothing plays" is good).
- Check that no criterion depends on a limit listed above without being marked as manual.
- Run `grep -nP '\x{2014}' tasks/prd-*.md` on the new file: there must be no em dash.
- Tell the user the path of the file, the decisions you took beyond their answers, and what was left unverified.
