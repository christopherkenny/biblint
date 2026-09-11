# biblint-syntax

`biblint-syntax` parses BibTeX into a public, source-aware data model.

Use this crate when you need to inspect entries, fields, comments, special
commands, or raw source programmatically. The parser is deliberately forgiving:
material it cannot structure remains visible in `Document::items`, and parse
problems are collected in `Document::errors`.

Most users who want to lint or format files should start with the
[`biblint-core`](https://crates.io/crates/biblint-core) library or the
[`biblint`](https://crates.io/crates/biblint) command-line tool.

```rust
use biblint_syntax::parse;

let document = parse("@article{key, title = {A title}}");

assert!(document.errors.is_empty());
assert_eq!(document.items.len(), 1);
```

The AST is intended for BibTeX tools that need source ranges and preservation
of unparsed material. It is not a semantic bibliography database or resolver.
