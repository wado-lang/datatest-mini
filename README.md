# datatest-mini [![crates.io](https://img.shields.io/crates/v/datatest-mini.svg)](https://crates.io/crates/datatest-mini)

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

### Async tests

```rust
async fn run_async_test(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

datatest_mini::harness! {
    // Uses #[tokio::test] by default
    { test = run_async_test, root = "tests/fixtures", pattern = r"\.txt$", async },
    // Custom attribute
    { test = run_async_test, root = "tests/fixtures", pattern = r"\.txt$",
      async, attr = r#"tokio::test(flavor = "multi_thread")"# },
}
```

The `attr` parameter can also be used without `async` for sync tests with custom attributes:

```rust
datatest_mini::harness! {
    { test = run_test, root = "tests/fixtures", pattern = r"\.txt$", attr = "googletest::test" },
}
```

## Parameters

| Parameter | Description |
|-----------|-------------|
| `test`    | Test function with signature `fn(&Path, &str) -> Result<(), Box<dyn Error>>` |
| `root`    | Path to the fixture directory (relative to `Cargo.toml`) |
| `pattern` | Regex pattern matched against relative file paths |
| `async`   | *(optional)* Generate `async fn` tests. Defaults to `#[tokio::test]` |
| `attr`    | *(optional)* Custom test attribute (e.g., `"tokio::test(flavor = \"multi_thread\")"`) |

## License

MIT
