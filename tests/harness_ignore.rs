//! Integration test for `ignore_if_env` / `ignore_unless_env`.
//!
//! These params are evaluated at *compile* time and emit a real `#[ignore]`, so
//! the test relies on two env vars whose presence is deterministic while the
//! macro expands under `cargo test`:
//! - `CARGO_MANIFEST_DIR` is always set.
//! - `DATATEST_MINI_NEVER_SET` is never set.
//!
//! Each entry uses a distinct test function (= distinct module) to avoid name
//! collisions. "must run" functions assert; "must ignore" functions panic if
//! ever executed — under the default `cargo test` they are `#[ignore]`d and so
//! never run, which is exactly what proves the attribute was emitted.
//!
//! NOTE: by design, running *this crate's* suite with `--include-ignored` will
//! execute the "must ignore" tests and fail — that failure is the proof they
//! were genuinely ignored, not absent.

use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_ran(path: &Path, content: &str) -> TestResult {
    assert!(path.exists(), "path should exist: {}", path.display());
    assert!(!content.is_empty(), "file should have content");
    Ok(())
}

fn ignore_if_set(_path: &Path, _content: &str) -> TestResult {
    panic!("ignore_if_env with a set var should have been #[ignore]d");
}

fn ignore_if_unset(path: &Path, content: &str) -> TestResult {
    assert_ran(path, content)
}

fn ignore_unless_unset(_path: &Path, _content: &str) -> TestResult {
    panic!("ignore_unless_env with an unset var should have been #[ignore]d");
}

fn ignore_unless_set(path: &Path, content: &str) -> TestResult {
    assert_ran(path, content)
}

fn ignore_if_array_one_set(_path: &Path, _content: &str) -> TestResult {
    panic!("ignore_if_env array with one var set should have been #[ignore]d");
}

fn ignore_unless_array_one_set(path: &Path, content: &str) -> TestResult {
    assert_ran(path, content)
}

async fn async_ignored(_path: &Path, _content: &str) -> TestResult {
    panic!("async #[ignore] (with #[tokio::test]) should not run");
}

async fn async_runs(path: &Path, content: &str) -> TestResult {
    tokio::task::yield_now().await;
    assert_ran(path, content)
}

datatest_mini::harness! {
    // ignore_if_env, var set -> ignored (panic must not fire)
    { test = ignore_if_set, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_if_env = "CARGO_MANIFEST_DIR" },
    // ignore_if_env, var unset -> runs (assert fires)
    { test = ignore_if_unset, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_if_env = "DATATEST_MINI_NEVER_SET" },
    // ignore_unless_env, var unset -> ignored
    { test = ignore_unless_unset, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_unless_env = "DATATEST_MINI_NEVER_SET" },
    // ignore_unless_env, var set -> runs
    { test = ignore_unless_set, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_unless_env = "CARGO_MANIFEST_DIR" },
    // ignore_if_env array, one var set -> ignored (OR)
    { test = ignore_if_array_one_set, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_if_env = ["DATATEST_MINI_NEVER_SET", "CARGO_MANIFEST_DIR"] },
    // ignore_unless_env array, one var set -> runs (OR)
    { test = ignore_unless_array_one_set, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      ignore_unless_env = ["DATATEST_MINI_NEVER_SET", "CARGO_MANIFEST_DIR"] },
    // #[ignore] composes with #[tokio::test]: ignored async test must not run
    { test = async_ignored, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      async, ignore_if_env = "CARGO_MANIFEST_DIR" },
    // async test that should run
    { test = async_runs, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      async, ignore_unless_env = "CARGO_MANIFEST_DIR" },
}
