# biblint

`biblint` is a deterministic BibTeX linter and formatter for local projects and CI.
It checks syntax and bibliography quality, normalizes formatting, and keeps the policy in a version-controlled `biblint.toml` file so everyone on a project uses the same rules.

## Install

You need Rust 1.88 or newer.
Install the latest source directly from GitHub:

```console
cargo install --git https://github.com/christopherkenny/biblint.git --package biblint --bin biblint
```

To work on biblint itself, clone the repository and install the local checkout:

```console
git clone https://github.com/christopherkenny/biblint.git
cd biblint
cargo install --path crates/biblint
```

## Quick start

Run these commands from the root of a project containing one or more `.bib` files.

First, inspect the project.
This reports syntax errors and the default lint rules without changing any files:

```console
biblint check .
```

Next, preview the formatter's changes.
`--diff` prints a unified diff and leaves the files untouched:

```console
biblint format . --diff
```

When the diff looks right, apply the configured formatting profile:

```console
biblint format .
```

For a conservative automated pass, `check --format --fix` writes only safe fixes.
Configured transforms that can change content or citation identity are skipped unless you also pass `--unsafe-fixes`.

## Choose a command

Use `check` for diagnostics and `format` when you want to produce formatted files.

| Command | Purpose                                           | Writes files? |
| ------- | ------------------------------------------------- | :-----------: |
| `biblint check .`    | Check syntax and default-enabled rules.           | No            |
| `biblint check . --format`    | Include a formatting diagnostic.                  | No            |
| `biblint check . --format --fix`    | Apply safe fixes, including formatting.           | Yes           |
| `biblint format .`    | Apply the configured formatting profile directly. | Yes           |
| `biblint format . --check`    | Report files that need formatting; useful in CI.  | No            |
| `biblint format . --diff`    | Preview the formatting changes as a diff.         | No            |

The path is optional and defaults to `.`.
A directory contributes `.bib` files recursively; an explicit file is checked on its own.
Use `-` to read one BibTeX document from standard input.

For machine-readable diagnostics, ask `check` for JSON:

```console
biblint check . --output json
```

To see the available rules or the details of one rule:

```console
biblint rule
biblint rule duplicate_key
```

## CI and exit status

Both commands return a useful status for automation:

| Status | Meaning                                                                          |
| :----: | -------------------------------------------------------------------------------- |
| `0`   | The command completed with no reported issues or pending changes.                |
| `1`   | Diagnostics were found, or `format --check`/`format --diff` found changes.                              |
| `2`   | A syntax/configuration/usage error occurred, or a diagnostic has error severity. |

A common CI gate is:

```console
biblint check . --format
```

It checks the configured rules and formatting without modifying the checkout.

## Configuration

Commit `biblint.toml` at the repository root.
Without `--config`, biblint searches upward from the first input path, so the same policy is found from subdirectories.
Use `--config path/to/other.toml` when a command needs a different policy.

This is a small starting point: the file-selection settings keep generated and vendored bibliographies out of the normal scan, while `extend-select` opts into checks that are not enabled by default.

```toml
include = ["**/*.bib"]
exclude = ["vendor/**", "generated/**"]

[lint]
extend-select = ["duplicate_doi", "key_format"]

[lint.key-format]
style = "better-bibtex"

[format]
sort = ["key"]
sort-fields = ["title", "author", "year"]
```

The default formatter changes layout, entry-type spelling, and field-name spelling while preserving value spelling.
Value transforms, sorting, duplicate cleanup, and citekey checks are choices you make in the configuration.
See [Configuration](docs/configuration.qmd) and [Formatting](docs/formatting.qmd) for the complete option set.

## Citekey generation and Markdown updates

`key_format` is opt-in because changing a citation key can break references in other files.
It reports a suggested key but does not rename existing keys.
`generate-keys = true` turns the same formula into a formatter transform and requires `--unsafe-fixes` when used with `check --format --fix`.

If keys change, one related Markdown, Quarto, or R Markdown document can be updated in the same operation.
The input must be one explicit BibTeX file, and the document must end in `.md`, `.qmd`, or `.Rmd`:

```console
biblint check references.bib --format --fix --unsafe-fixes --update-markdown manuscript.qmd
```

Bracketed and textual Pandoc citations are updated; code, comments, HTML tags, and email-like addresses are left alone.
See [Citekey generation](docs/key-generation.md) for formula syntax and collision behavior.

## More documentation

The [documentation site](docs/index.qmd) covers the complete workflow:

- [Configuration](docs/configuration.qmd) — file discovery, rule selection, and policy settings
- [Formatting](docs/formatting.qmd) — layout, ordering, and value transforms
- [Rules](docs/rules.qmd) — diagnostics, suppressions, and project-level checks
- [Citekey generation](docs/key-generation.md) — supported formula syntax and Markdown updates
- [Development](docs/development.qmd) — workspace checks and site rendering
