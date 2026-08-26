# biblint

`biblint` is a deterministic Rust linter and formatter for BibTeX. It is
designed for the post-edit pass in agentic workflows: formatting fixes are
repeatable, while semantic observations such as duplicate records remain
visible diagnostics.

The [documentation](docs/index.qmd) covers the general workflow, configuration,
formatting, rules, and citekey generation.

## Status

The first pass includes a forgiving parser, canonical entry formatting,
comments and special-entry handling, duplicate-key/field checks, optional DOI,
citation, and abstract duplicate checks, unsupported-LaTeX-escape notes, and a
broad set of cleanup transforms. Unknown source is
preserved and reported as an `unsupported_construct` note instead of being
discarded.

Implemented formatting options include field alignment and indentation,
lowercasing, sorting, curly/numeric/month normalization, duplicate and empty
field cleanup, comments, page ranges, URL underscores, key generation, author
limits, wrapping, brace transforms, escaping, and duplicate merging.

The default format profile normalizes layout whitespace, entry types, and field
names: two-space field indentation, lowercase identifiers, and a single space
on either side of `=`. Value spelling, escaping, cleanup, sorting, and semantic
transforms are opt-in through configuration.

## Usage

```console
biblint check references.bib
biblint check references.bib --fix
biblint check . --output json
biblint format references.bib --check
biblint format references.bib --diff
biblint rule duplicate_key
biblint rule key_format
```

Both `check` and `format` read standard input when the path is `-`.

The CLI controls execution only. Formatting policy, rule selection, discovery,
and exceptions are read from `biblint.toml`, discovered from the input path or
passed explicitly with `--config`.

```toml
include = ["**/*.bib"]
exclude = ["vendor/**"]

[lint]
extend-select = ["duplicate_doi", "duplicate_citation"]
ignore = ["unsupported_escape"]

[lint.key-format]
style = "better-bibtex"

[format.key-generation]
formula = "auth.lower + year + shorttitle(1,0)"

[lint.per-file-ignores]
"generated/**" = ["formatting"]

[format]
indent = 2
sort = ["key"]
sort-fields = ["title", "author", "year"]
escape = true
remove-duplicate-fields = true
```

All formatter controls, including cleanup and semantic transforms, are
available as `[format]` settings rather than one-off command-line switches.
Semantic transforms remain opt-in and require `--unsafe-fixes` when applied by
`check --fix`.

`check --fix` applies safe canonical formatting and then checks again. Content
and identity transforms such as key generation, duplicate merging, field
removal, brace removal, and author truncation require
`--unsafe-fixes`. Duplicate and missing-key diagnostics are intentionally not
fixed automatically because choosing the correct record or key requires
context.

The `key_format` rule is opt-in because changing a citation key can break
citation references in other files. Its `better-bibtex` style follows
an independently evaluated formula written in Better BibTeX syntax, which
defaults to the Google Scholar-style `auth.lower + year + shorttitle(1,0)`:
it uses the first creator surname
(falling back through editor, translator, and collaborator), the publication
year, and the first significant title word. Diacritics are folded to ASCII,
punctuation is removed, and repeated generated keys receive alphabetic
suffixes such as `a`, `b`, and `aa`.
Diagnostics never rename an existing key. Setting `generate-keys = true` under
`[format]` uses the same generator as an explicit unsafe transform. The
`[format.key-generation].formula` setting accepts the deterministic BibTeX
subset of Better BibTeX's formula language: direct field access, quoted
strings, `+` composition, `||` fallbacks, top-level `;`/`|` alternates,
`&&` conditions, ternaries, length comparisons, creator/title/year/page
functions, `extra(...)`, and filters such as `clean`, `lower`, `upper`,
`capitalize`, `select`, `substring`, `skipwords`, `transliterate`, and `len`.
Unsupported Zotero-only functions
such as `group`, `library`, and `zotero` are configuration errors.
See [citekey generation and formula syntax](docs/key-generation.md) for the
compatibility boundary and the complete supported subset.


## Development

```console
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The documentation source is in `docs/` and can be rendered with
`quarto render docs`.

The `project-duplicates` fixture covers the project-level `check` path: keys,
DOIs, citation fingerprints, and abstract fingerprints duplicated across two
files must be reported together with a related first-use location.
