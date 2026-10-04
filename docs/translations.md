# Translations

UI strings are written in English in the code and marked as translatable (gettext, domain `vinheta`). `po/LINGUAS` lists `pt_BR`, and `po/pt_BR.po` must translate every string: the check "translations are complete" of `scripts/check.sh` fails otherwise.

After adding or changing a string:

```sh
meson compile -C build vinheta-pot            # writes po/vinheta.pot (git-ignored)
msgmerge --update --backup=none po/pt_BR.po po/vinheta.pot
# translate the new and the fuzzy entries of po/pt_BR.po, then:
scripts/check-translations.sh
scripts/screenshot.sh NAME --lang pt_BR ...   # read the result
```

- `xgettext` does not know Rust and reads the `.rs` files as C, with warnings. It has found every call so far; the check compares the number of `gettext(` and `ngettext(` calls of each file with the template.
- Product names ("Vinheta", "PipeWire") are not translated, the `{}` placeholders and the typographic quotes are kept, and an accelerator underscore must sit on a letter that no other item of the same menu uses.
- The desktop file and the metainfo are merged with the translations at build time. After a change of the `.po` alone, touch their `.in` files or the build keeps the old ones.
- The pictures of the metainfo (`data/screenshots/`) are made by `scripts/metainfo-screenshots.sh`. `preview.png` (1280x640, the picture of the README and the social preview of the repository) is composed by hand from `main.png` and the icon: redo it when the main window changes.
