# biblint

`biblint` is a BibTeX linter and formatter.
Commit a `biblint.toml` file so coauthors and CI use the same policy.

## Installation

Requires Rust 1.88+.

Install from the Git repository:

```console
cargo install --git https://github.com/christopherkenny/biblint.git --package biblint --bin biblint
```

Or clone and install from a local checkout:

```console
git clone https://github.com/christopherkenny/biblint.git
cd biblint
cargo install --path crates/biblint
```

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

## Configuration

Formatting policy, rule selection, file discovery, and exceptions live in `biblint.toml`.
Without `--config`, biblint searches upward from the input path.

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
escape = true

[format.key-generation]
formula = "auth.lower + year + shorttitle(1,0)"
```

The default profile normalizes layout, entry types, and field names while preserving value spelling.
Additional cleanup, sorting, duplicate checks, value transforms, and citekey checks are opt-in.

The `key_format` rule is opt-in because changing a citation key can break citation references in other files.
Its default formula is `auth.lower + year + shorttitle(1,0)`; see [citekey generation](docs/key-generation.md) for customization.
`generate-keys = true` applies the formula as a formatter transform and requires `--unsafe-fixes` with `check --fix`.
