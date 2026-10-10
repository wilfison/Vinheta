# Translations

UI strings are written in English in the code and marked as translatable (gettext, domain `vinheta`). `po/LINGUAS` lists the languages (`de`, `es`, `fr`, `pt`, `pt_BR`), and the `.po` file of every one of them must translate every string: the check "translations are complete" of `scripts/check.sh` fails otherwise.

After adding or changing a string:

```sh
scripts/check-translations.sh --update        # merges the strings into po/*.po and lists the ones to translate
# translate them in every po/*.po, then:
scripts/check-translations.sh
scripts/screenshot.sh NAME --lang pt_BR ...   # read the result, in each language that matters
```

- To add a language, copy `po/pt_BR.po` (or the one closest to it: `pt` came from `pt_BR`) to `po/LANG.po`, set its `Language`, `Language-Team`, and `Plural-Forms` headers, translate every string, and add `LANG` to `po/LINGUAS`, which `po/meson.build` (the build reconfigures itself when it changes), `scripts/check-translations.sh`, and the Debian package all read. Nothing else lists the languages; the metainfo and the desktop file are translated from the same `.po`. Mention the language in `CHANGELOG.md` and `docs/overview.md`.

- `--lang` of the scripts needs a locale generated on the machine (`locale -a`): `virtual_session` of `scripts/dev-common.sh` takes the one of the language or of one of its regions, and otherwise any other, since `LANGUAGE` picks the strings anyway but is ignored under the C locale. Every language is read fine with `pt_BR.utf8` alone.

- Accelerators: the underscore of each menu goes on a letter no other item of the same popover uses, which differs per language (`pt` has "Adicionar pas_ta" because "_Preferências" takes the P).

- `pt_BR` is by the author; `de`, `es`, `fr`, and `pt` were written with an AI assistant and are not reviewed by native speakers yet, which the README says, with an invitation to review them.

- `--update` merges without fuzzy matching: `msgmerge` alone proposed "Fundo" (the translation of "Background") for "Adjust Background", a guess that is easy to keep by mistake. It also drops the obsolete strings.

- `xgettext` does not know Rust and reads the `.rs` files as C, with warnings. It has found every call so far; the check compares the number of `gettext(` and `ngettext(` calls of each file with the template.
- Product names ("Vinheta", "PipeWire") are not translated, the `{}` placeholders and the typographic quotes are kept, and an accelerator underscore must sit on a letter that no other item of the same menu uses.
- The desktop file and the metainfo are merged with the translations at build time. After a change of the `.po` alone, touch their `.in` files or the build keeps the old ones.
- The pictures of the metainfo (`data/screenshots/`) are made by `scripts/metainfo-screenshots.sh` (all of them, or the ones named: `main`, `preferences`, `call-guide`; a picture whose pixels did not change is left alone). `preview.png` (1280x640, the picture of the README and the social preview of the repository) is composed by hand from `main.png` and the icon: redo it when the main window changes.
