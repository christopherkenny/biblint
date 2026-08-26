# Citekey generation and Better BibTeX-compatible formulas

`biblint` can check citation keys and can generate deterministic key suggestions from BibTeX entries.
The formula language is intentionally familiar to users of Better BibTeX, but the implementation is independent.

## Scope and compatibility

This is not Better BibTeX embedded in `biblint`:

- `biblint` is implemented in Rust and does not call Better BibTeX, Zotero, or Node at runtime.
- Formulas are evaluated from the parsed BibTeX entry only.
  There is no Zotero item, collection, library, or group object available.
- The supported syntax is a deterministic subset of the Better BibTeX formula language.
  Matching syntax does not promise byte-for-byte compatibility for every Better BibTeX formula or every Zotero export.
- Unsupported functions and filters are rejected while loading configuration; they are never silently approximated.

The goal is a useful interoperability surface for formulas whose inputs can be recovered from a `.bib` file, not a claim to implement all of Better BibTeX.

## Default formula

The built-in default is the Google Scholar-style profile:

```text
auth.lower + year + shorttitle(1,0)
```

It means:

1. use the first creator surname, falling back through `author`, `editor`, `translator`, and `collaborator`;
2. lowercase that surname;
3. append the first four digits found in `year`;
4. append the first significant word of `title`.

`shorttitle` removes stopwords such as `a`, `an`, and `the` before counting words.
The `0` means that biblint does not capitalize the title fragment; it preserves the spelling found in the entry.

For example:

```bibtex
@article{old,
  author = {Smith, John},
  title = {A Study of Things},
  year = {2024}
}
```

produces the suggestion `smith2024Study`.
With `The effects of NASA models` as the title, the title fragment is `effects`, so the suggestion is `smith2024effects`.

Generated collisions receive deterministic alphabetic suffixes: `a`, `b`, …, `z`, `aa`, and so on.
Entries without enough key material are left without a suggestion.

## Configuration

Keep the formula in the repository's `biblint.toml` so every machine and CI run uses the same policy:

```toml
[lint]
extend-select = ["key_format"]

[lint.key-format]
style = "better-bibtex"

[format]
generate-keys = true

[format.key-generation]
formula = "auth.lower + year + shorttitle(1,0)"
```

The formula setting is optional because the example is the default.
The `key_format` rule is opt-in: it reports when an existing key differs from the configured suggestion, but it does not rename the key automatically because renaming can break citations in other files.

`generate-keys = true` enables key replacement as a formatter transform.
With `check --fix`, key generation is an identity-changing transform and therefore requires `--unsafe-fixes`:

```console
biblint check references.bib --fix --unsafe-fixes
biblint format references.bib
```

## Supported formula syntax

Formula names are case-insensitive.
The evaluator supports:

- direct BibTeX fields, such as `title`, `doi`, and `shortauthor`;
- quoted strings and numbers, for example `"-"` and `2024`;
- concatenation with `+`;
- fallbacks with `||`, and top-level alternate patterns separated by `;` or `|`;
- conditional composition with `&&`;
- ternaries such as `language == "en" ? "eng" : ""`;
- length comparisons using `==`, `!=`, `<`, `<=`, `>`, or `>=`;
- chained filters such as `auth.lower.clean`;
- positional and named function arguments, such as `substring(start=1,n=3)`.

The implemented entry functions are:

- creators: `auth`, `authAuthEa`, `authEtAl`, `authEtal2`, `authForeIni`, `authIni`, `authorIni`, `authorLast`, `authors`, `authorsAlpha`, `authorsn`, and `authshort`;
- entry fields and metadata: `date`, `extra`, `firstpage`, `journal`, `language`, `lastpage`, `month`, `origdate`, `origyear`, `shortyear`, `title`, `type`, and `year`;
- title fragments: `shorttitle` and `veryshorttitle`.

The implemented filters are `abbr`, `alphanum`, `ascii`, `capitalize`, `clean`, `condense`, `default`, `discard`, `len`, `lower`, `nopunct`, `nopunctordash`, `numeric`, `postfix`, `prefix`, `replace`, `select`, `skipwords`, `substring`, `transliterate`, and `upper`.

For example, this formula uses a short author field from Better BibTeX's `extra` data when it is present and otherwise falls back to the BibTeX author:

```text
extra('tex.shortauthor').clean.lower || auth.lower
```

`extra(...)` reads matching `key: value` lines from the BibTeX `extra` field; it does not query Zotero metadata.

## Unsupported Better BibTeX features

Zotero-only constructs such as these are configuration errors:

```text
group('Methods') + auth
library + auth
zotero + year
```

The same applies to any function or filter outside the supported lists above.
This fail-closed behavior is deliberate: a plausible but different key is more dangerous than a clear configuration error.

## Determinism and safety

The same BibTeX input and the same committed configuration produce the same suggestions regardless of machine, working directory, or whether the run is performed locally or in CI.
Key suggestions are used consistently by both the `key_format` diagnostic and `generate-keys` formatting.
Existing keys are reserved during generation, and collision suffixes are chosen in document order so results remain stable.
