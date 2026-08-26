# Citekey generation

`biblint` can check existing citation keys and generate deterministic suggestions from BibTeX entries.
The `better-bibtex` style uses a deterministic subset of Better BibTeX's formula syntax and reads only data in the `.bib` file.

Unsupported functions and filters are rejected when the configuration is loaded.

## Default formula

The built-in formula is:

```text
auth.lower + year + shorttitle(1,0)
```

It:

1. uses the first creator surname, falling back through `author`, `editor`, `translator`, and `collaborator`
2. lowercases that surname
3. appends the first four digits found in `year`
4. appends the first significant word of `title`

`shorttitle` removes common stopwords before counting words.
The `0` preserves the title fragment's original capitalization.

For example:

```bibtex
@article{old,
  author = {Smith, John},
  title = {A Study of Things},
  year = {2024}
}
```

produces the suggestion `smith2024Study`.
By default, collisions receive alphabetic suffixes such as `a`, `b`, and `aa`.
Entries without enough key material are left unchanged.

## Configuration

Keep the formula in the repository's `biblint.toml`:

```toml
[lint]
extend-select = ["key_format"]

[lint.key-format]
style = "better-bibtex"

[format]
generate-keys = true

[format.key-generation]
formula = "auth.lower + year + shorttitle(1,0)"
collision-suffix = "alphabetic"
```

The formula is optional because the example is the default.
`key_format` reports differences but does not rename existing keys.
`generate-keys = true` enables replacement as a formatter transform.
With `check --format --fix`, it requires `--unsafe-fixes`:

```console
biblint check references.bib --format --fix --unsafe-fixes
biblint format references.bib
```

For author/year keys that reserve the first suffix letter, use `collision-suffix = "skip-a"`.
The following formula uses the full surname for one creator and the first three letters of up to three creators otherwise:

```toml
[format.key-generation]
formula = "auth(0,m=2) ? auth(3,m=1) + auth(3,m=2) + auth(3,m=3) + shortyear : auth + shortyear"
collision-suffix = "skip-a"
```

Creator lists follow standard BibTeX name syntax: separate creators with the word `and`.

## Supported formula syntax

Formula names are case-insensitive.
The evaluator supports:

- direct BibTeX fields, such as `title`, `doi`, and `shortauthor`
- quoted strings and numbers, for example `"-"` and `2024`
- concatenation with `+`
- fallbacks with `||`, and top-level alternate patterns separated by `;` or `|`
- conditional composition with `&&`
- ternaries such as `language == "en" ? "eng" : ""`
- length comparisons using `==`, `!=`, `<`, `<=`, `>`, or `>=`
- chained filters such as `auth.lower.clean`
- positional and named function arguments, such as `substring(start=1,n=3)`

The implemented entry functions are:

- creators: `auth`, `authAuthEa`, `authEtAl`, `authEtal2`, `authForeIni`, `authIni`, `authorIni`, `authorLast`, `authors`, `authorsAlpha`, `authorsn`, and `authshort`
- entry fields and metadata: `date`, `extra`, `firstpage`, `journal`, `language`, `lastpage`, `month`, `origdate`, `origyear`, `shortyear`, `title`, `type`, and `year`
- title fragments: `shorttitle` and `veryshorttitle`

The implemented filters are `abbr`, `alphanum`, `ascii`, `capitalize`, `clean`, `condense`, `default`, `discard`, `len`, `lower`, `nopunct`, `nopunctordash`, `numeric`, `postfix`, `prefix`, `replace`, `select`, `skipwords`, `substring`, `transliterate`, and `upper`.

For example, this formula uses a short author field from Better BibTeX's `extra` data when it is present and otherwise falls back to the BibTeX author:

```text
extra('tex.shortauthor').clean.lower || auth.lower
```

`extra(...)` reads matching `key: value` lines from the BibTeX `extra` field.
Functions and filters outside the supported lists are configuration errors.

## Determinism and safety

The same BibTeX input and committed configuration produce the same suggestions.
Existing keys are reserved, and collision suffixes are assigned in document order.
