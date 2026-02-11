# datatest-mini

Minimal proc macro for generating test functions from fixture files. A lightweight drop-in replacement for [datatest-stable](https://crates.io/crates/datatest-stable).

## Usage

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
datatest-mini = "0.1"
```

Create a test harness (e.g., `tests/harness.rs`):

```rust
use std::path::Path;

fn run_test(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    // path: absolute path to the fixture file
    // content: file content embedded at compile time via include_str!
    Ok(())
}

datatest_mini::harness! {
    { test = run_test, root = "tests/fixtures", pattern = r"^[^/]+\.txt$" },
}
```

Each file matching the pattern generates a separate `#[test]` function, so you get individual pass/fail results per fixture.

### Multiple test sets

```rust
datatest_mini::harness! {
    { test = test_parsing, root = "tests/parse_fixtures", pattern = r"\.txt$" },
    { test = test_codegen, root = "tests/codegen_fixtures", pattern = r"\.wado$" },
}
```

## Parameters

| Parameter | Description |
|-----------|-------------|
| `test`    | Test function with signature `fn(&Path, &str) -> Result<(), Box<dyn Error>>` |
| `root`    | Path to the fixture directory (relative to `Cargo.toml`) |
| `pattern` | Regex pattern matched against relative file paths |

## License

MIT
