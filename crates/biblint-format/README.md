# biblint-format

`biblint-format` provides deterministic formatting and cleanup transforms for
parsed BibTeX documents.

Use this crate when you already have a `biblint_syntax::Document` and need
programmatic formatting, sorting, value transforms, or citekey suggestions.
It does not read files or discover configuration; those higher-level concerns
belong to [`biblint-core`](https://crates.io/crates/biblint-core).

```rust
use biblint_format::{FormatOptions, format_document};
use biblint_syntax::parse;

let document = parse("@article{key,title={A title}}");
let result = format_document(&document, &FormatOptions::default());

assert!(!result.output.is_empty());
```

Formatting returns a `FormatResult` containing the rendered document and any
citation-key changes. The input document is not modified.
