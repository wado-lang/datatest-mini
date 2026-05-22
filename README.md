# datatest-mini [![crates.io](https://img.shields.io/crates/v/datatest-mini.svg)](https://crates.io/crates/datatest-mini)

Minimal proc macro for generating test functions from fixture files. A lightweight drop-in replacement for [datatest-stable](https://crates.io/crates/datatest-stable).

## Usage

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
datatest-mini = "0.4"
```

Create a test harness (e.g., `tests/harness.rs`):

```rust
use std::path::Path;

fn run_test(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    // path: absolute path to the fixture file
    // content: file content read at runtime via std::fs::read_to_string
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
| `ignore_if_env`     | *(optional)* Env var name, or `["A", "B"]`. Mark the test `#[ignore]` if **any** is set |
| `ignore_unless_env` | *(optional)* Env var name, or `["A", "B"]`. Mark the test `#[ignore]` **unless any** is set |

### Ignoring tests by environment variable

```rust
datatest_mini::harness! {
    // Slow tests: run only in CI or when WADO_FULL_TEST is set, ignored otherwise.
    { test = slow_test, root = "tests/fixtures", pattern = r"\.txt$",
      ignore_unless_env = ["CI", "WADO_FULL_TEST"] },
    // Kill switch: ignore whenever DATATEST_MINI_SKIP is set.
    { test = run_test, root = "tests/fixtures", pattern = r"\.txt$",
      ignore_if_env = "DATATEST_MINI_SKIP" },
}
```

The env vars are read at **compile time** (macro expansion) and baked into a real
`#[ignore = "..."]`, so ignored tests show as `ignored` (not passing) and do no
file I/O. Run them on demand without rebuilding via `cargo test -- --ignored`
(only ignored) or `cargo test -- --include-ignored` (all).

Because the decision is made at compile time, changing an env var does **not**
automatically rebuild — re-expand the macro by `touch`ing the test file (the same
step you already need to pick up added/removed fixtures) or run `cargo clean`.
The auto-tracking API `proc_macro::tracked_env` is nightly-only.

## License

MIT
