//! Integration test for async support in datatest_mini::harness! macro

use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn run_async_fixture(path: &Path, content: &str) -> TestResult {
    assert!(path.exists(), "path should exist: {}", path.display());
    assert!(!content.is_empty(), "file should have content");
    // Verify we're actually in an async context
    tokio::task::yield_now().await;
    Ok(())
}

async fn run_async_fixture_mt(path: &Path, content: &str) -> TestResult {
    assert!(path.exists(), "path should exist: {}", path.display());
    assert!(!content.is_empty(), "file should have content");
    tokio::task::yield_now().await;
    Ok(())
}

datatest_mini::harness! {
    // async with default #[tokio::test]
    { test = run_async_fixture, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$", async },
    // async with custom attribute
    { test = run_async_fixture_mt, root = "tests/fixtures", pattern = r"^test_[^/]+\.txt$",
      async, attr = r#"tokio::test(flavor = "multi_thread")"# },
}
