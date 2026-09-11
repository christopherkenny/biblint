# biblint-core

`biblint-core` is the high-level library API for checking and formatting
BibTeX sources.

Use it for editor, CI, or application integrations that need configuration,
file discovery, diagnostics, formatting, or safe and unsafe fixes without
invoking a command-line process. It combines the parser and formatter crates.
The [`biblint`](https://crates.io/crates/biblint) binary is the command-line
frontend built on this API.

```rust
use biblint_core::{Settings, check_source};
use std::path::Path;

let checked = check_source(
    "@article{key, title = {A title}}",
    Path::new("references.bib"),
    &Settings::default(),
);

assert!(checked.parse.errors.is_empty());
```

`biblint-core` re-exports the formatting option and result types needed by its
source-level API. Lower-level AST and document transforms remain available
from [`biblint-syntax`](https://crates.io/crates/biblint-syntax) and
[`biblint-format`](https://crates.io/crates/biblint-format).
