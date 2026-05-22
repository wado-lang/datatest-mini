//! Minimal `proc_macro` for generating test functions from fixture files.
//!
//! This crate provides a drop-in replacement for datatest-stable.
//!
//! # Usage
//!
//! ```ignore
//! use std::path::Path;
//!
//! fn run_test(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
//!     // path: absolute path to the fixture file
//!     // content: file content read at runtime via std::fs::read_to_string
//!     Ok(())
//! }
//!
//! datatest_mini::harness! {
//!     { test = run_test, root = "tests/fixtures", pattern = r"^[^/]+\.wado$" },
//! }
//! ```
//!
//! # Async tests
//!
//! ```ignore
//! async fn run_async_test(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
//!     Ok(())
//! }
//!
//! datatest_mini::harness! {
//!     // Uses #[tokio::test] by default
//!     { test = run_async_test, root = "tests/fixtures", pattern = r"\.txt$", async },
//!     // Custom attribute
//!     { test = run_async_test, root = "tests/fixtures", pattern = r"\.txt$",
//!       async, attr = r#"tokio::test(flavor = "multi_thread")"# },
//! }
//! ```

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use regex::Regex;
use std::path::Path;

// TODO: Use `proc_macro::tracked_path` to track fixture directories when it
// stabilizes. This would allow detecting new/removed fixture files without
// build.rs. Currently, adding or removing fixture files may not trigger
// recompilation automatically.

/// Generate a test harness for fixture files.
///
/// # Syntax
///
/// ```ignore
/// datatest_mini::harness! {
///     { test = test_fn, root = "path/to/fixtures", pattern = r"pattern" },
///     { test = async_fn, root = "path/to/fixtures", pattern = r"pattern", async },
///     { test = async_fn, root = "path/to/fixtures", pattern = r"pattern",
///       async, attr = r#"tokio::test(flavor = "multi_thread")"# },
/// }
/// ```
///
/// Multiple test sets can be specified by adding more entries.
/// The test function name is used as the module name (e.g., `test_fn::file_name`).
///
/// # Optional parameters
///
/// - `async`: Generate `async fn` tests. Defaults to `#[tokio::test]` attribute.
/// - `attr`: Custom test attribute (e.g., `"tokio::test(flavor = \"multi_thread\")"`).
///   When used without `async`, generates sync tests with the specified attribute.
/// - `ignore_if_env`: Env var name, or array of names. Mark the test `#[ignore]`
///   if ANY of them is set.
/// - `ignore_unless_env`: Env var name, or array of names. Mark the test
///   `#[ignore]` unless ANY of them is set.
///
/// The env vars are read at *compile* time (macro expansion) and baked into a
/// real `#[ignore = "..."]` attribute, so ignored tests show as `ignored` (not
/// passing) and do no file I/O. Run them on demand with
/// `cargo test -- --ignored` (only ignored) or `--include-ignored` (all) without
/// rebuilding. Because the decision is compile-time, changing the env requires
/// re-expanding the macro — `touch` the test file (or `cargo clean`) so Cargo
/// re-runs it; `proc_macro::tracked_env` (which would auto-rebuild) is nightly
/// only.
///
/// ```ignore
/// datatest_mini::harness! {
///     // Ignored unless CI or WADO_FULL_TEST is set at build time.
///     { test = slow_test, root = "tests/fixtures", pattern = r"\.txt$",
///       ignore_unless_env = ["CI", "WADO_FULL_TEST"] },
///     // Ignored whenever DATATEST_MINI_SKIP is set at build time.
///     { test = run_test, root = "tests/fixtures", pattern = r"\.txt$",
///       ignore_if_env = "DATATEST_MINI_SKIP" },
/// }
/// ```
///
/// # Panics
///
/// Panics if:
/// - `CARGO_MANIFEST_DIR` environment variable is not set
/// - The fixture directory does not exist
/// - The pattern regex is invalid
/// - The fixture directory cannot be read
#[proc_macro]
pub fn harness(input: TokenStream) -> TokenStream {
    let entries = parse_harness_entries(input);

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    let mut all_tests: Vec<TestEntry> = Vec::new();

    for entry in entries {
        let full_path = Path::new(&manifest_dir).join(&entry.root);
        assert!(
            full_path.exists(),
            "fixture directory does not exist: {}",
            full_path.display()
        );

        let regex = Regex::new(&entry.pattern)
            .unwrap_or_else(|e| panic!("invalid pattern '{}': {}", entry.pattern, e));

        // Decide whether this entry's tests are ignored, reading the env vars at
        // macro-expansion (compile) time. The decision is baked into the
        // generated `#[ignore]`; changing the env requires re-expanding the
        // macro (e.g. `touch` the test file or `cargo clean`).
        let ignore_reason = compute_ignore_reason(&entry.ignore_if_env, &entry.ignore_unless_env);

        // Use test function name as module name
        collect_matching_files(
            &full_path,
            &full_path,
            &regex,
            &entry.test_fn,
            entry.is_async,
            &entry.attr,
            &ignore_reason,
            &mut all_tests,
        );
    }

    generate_test_functions(&all_tests)
}

struct HarnessEntry {
    test_fn: String,
    root: String,
    pattern: String,
    is_async: bool,
    attr: Option<String>,
    /// Env var names; mark the test `#[ignore]` if ANY of them is set at
    /// compile time.
    ignore_if_env: Vec<String>,
    /// Env var names; mark the test `#[ignore]` unless ANY of them is set at
    /// compile time.
    ignore_unless_env: Vec<String>,
}

fn parse_harness_entries(input: TokenStream) -> Vec<HarnessEntry> {
    let mut entries = Vec::new();
    let mut iter = input.into_iter().peekable();

    while let Some(token) = iter.next() {
        if let TokenTree::Group(group) = token
            && group.delimiter() == Delimiter::Brace
            && let Some(entry) = parse_single_entry(group.stream())
        {
            entries.push(entry);
        }
        // Skip commas between entries
        if let Some(TokenTree::Punct(p)) = iter.peek()
            && p.as_char() == ','
        {
            iter.next();
        }
    }

    entries
}

fn parse_single_entry(stream: TokenStream) -> Option<HarnessEntry> {
    let mut test_fn = None;
    let mut root = None;
    let mut pattern = None;
    let mut is_async = false;
    let mut attr = None;
    let mut ignore_if_env = Vec::new();
    let mut ignore_unless_env = Vec::new();

    let mut iter = stream.into_iter().peekable();

    while let Some(token) = iter.next() {
        if let TokenTree::Ident(ident) = token {
            let key = ident.to_string();

            // Handle bare `async` flag (no `= value`)
            if key == "async" {
                is_async = true;
                // Check if followed by `= "..."` for custom attribute
                if let Some(TokenTree::Punct(p)) = iter.peek()
                    && p.as_char() == '='
                {
                    // Consume `=` — this `async` has a value, treat as attr
                    iter.next();
                    if let Some(TokenTree::Literal(lit)) = iter.next() {
                        attr = Some(parse_string_literal(&lit));
                    }
                }
                // Skip comma
                if let Some(TokenTree::Punct(p)) = iter.peek()
                    && p.as_char() == ','
                {
                    iter.next();
                }
                continue;
            }

            // Skip '='
            if let Some(TokenTree::Punct(p)) = iter.next()
                && p.as_char() != '='
            {
                continue;
            }

            // Get value
            match key.as_str() {
                "test" => {
                    if let Some(TokenTree::Ident(val)) = iter.next() {
                        test_fn = Some(val.to_string());
                    }
                }
                "root" => {
                    if let Some(TokenTree::Literal(lit)) = iter.next() {
                        root = Some(parse_string_literal(&lit));
                    }
                }
                "pattern" => {
                    if let Some(TokenTree::Literal(lit)) = iter.next() {
                        pattern = Some(parse_string_literal(&lit));
                    }
                }
                "attr" => {
                    if let Some(TokenTree::Literal(lit)) = iter.next() {
                        attr = Some(parse_string_literal(&lit));
                    }
                }
                "ignore_if_env" => {
                    ignore_if_env = parse_env_list(iter.next());
                }
                "ignore_unless_env" => {
                    ignore_unless_env = parse_env_list(iter.next());
                }
                _ => {}
            }

            // Skip comma
            if let Some(TokenTree::Punct(p)) = iter.peek()
                && p.as_char() == ','
            {
                iter.next();
            }
        }
    }

    Some(HarnessEntry {
        test_fn: test_fn?,
        root: root?,
        pattern: pattern?,
        is_async,
        attr,
        ignore_if_env,
        ignore_unless_env,
    })
}

fn parse_string_literal(lit: &Literal) -> String {
    let s = lit.to_string();
    // Handle both regular strings "..." and raw strings r"..."
    if s.starts_with("r\"") || s.starts_with("r#") {
        // Raw string: r"..." or r#"..."#
        let s = s.trim_start_matches('r');
        let hash_count = s.chars().take_while(|&c| c == '#').count();
        let start = hash_count + 1; // Skip '#'s and opening '"'
        let end = s.len() - hash_count - 1; // Skip closing '"' and '#'s
        s[start..end].to_string()
    } else {
        // Regular string: "..."
        s.trim_matches('"').to_string()
    }
}

/// Parse an `ignore_if_env` / `ignore_unless_env` value: either a single string
/// literal `"VAR"` or an array of string literals `["A", "B"]`. Returns the
/// list of env var names (commas inside the array are ignored).
fn parse_env_list(token: Option<TokenTree>) -> Vec<String> {
    match token {
        Some(TokenTree::Literal(lit)) => vec![parse_string_literal(&lit)],
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => group
            .stream()
            .into_iter()
            .filter_map(|t| match t {
                TokenTree::Literal(lit) => Some(parse_string_literal(&lit)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

struct TestEntry {
    module_name: String,
    test_name: String,
    path: String,
    test_fn: String,
    is_async: bool,
    attr: Option<String>,
    /// `Some(reason)` to emit `#[ignore = "reason"]`; `None` for a normal test.
    ignore_reason: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn collect_matching_files(
    base_path: &Path,
    current_path: &Path,
    pattern: &Regex,
    test_fn: &str,
    is_async: bool,
    attr: &Option<String>,
    ignore_reason: &Option<String>,
    tests: &mut Vec<TestEntry>,
) {
    let entries = match std::fs::read_dir(current_path) {
        Ok(entries) => entries,
        Err(e) => panic!("failed to read directory {}: {}", current_path.display(), e),
    };

    let mut entries: Vec<_> = entries.filter_map(std::result::Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            collect_matching_files(
                base_path,
                &path,
                pattern,
                test_fn,
                is_async,
                attr,
                ignore_reason,
                tests,
            );
            continue;
        }

        if !path.is_file() {
            continue;
        }

        // Get relative path from base for pattern matching
        let rel_path = path
            .strip_prefix(base_path)
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Check if pattern matches
        if !pattern.is_match(&rel_path) {
            continue;
        }

        // Generate test name from relative path
        let test_name = rel_path.replace([std::path::MAIN_SEPARATOR, '-', '.'], "_");

        tests.push(TestEntry {
            module_name: test_fn.to_string(),
            test_name,
            path: path.display().to_string(),
            test_fn: test_fn.to_string(),
            is_async,
            attr: attr.clone(),
            ignore_reason: ignore_reason.clone(),
        });
    }
}

/// Decide, at macro-expansion (compile) time, whether an entry's tests should be
/// marked `#[ignore]`, and with what reason.
///
/// - `ignore_if_env`: ignore when ANY listed var is set.
/// - `ignore_unless_env`: ignore when NONE of the listed vars is set.
///
/// Returns `None` (run normally) when no condition triggers.
fn compute_ignore_reason(ignore_if_env: &[String], ignore_unless_env: &[String]) -> Option<String> {
    let mut reasons: Vec<String> = Vec::new();

    let set_vars: Vec<&str> = ignore_if_env
        .iter()
        .filter(|v| std::env::var_os(v).is_some())
        .map(String::as_str)
        .collect();
    if !set_vars.is_empty() {
        reasons.push(format!("unset {} to run", set_vars.join("/")));
    }

    if !ignore_unless_env.is_empty()
        && ignore_unless_env
            .iter()
            .all(|v| std::env::var_os(v).is_none())
    {
        reasons.push(format!("set {} to run", ignore_unless_env.join("/")));
    }

    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    }
}

fn generate_test_functions(tests: &[TestEntry]) -> TokenStream {
    use std::collections::BTreeMap;

    // Group tests by module name
    let mut modules: BTreeMap<String, Vec<&TestEntry>> = BTreeMap::new();
    for test in tests {
        modules
            .entry(test.module_name.clone())
            .or_default()
            .push(test);
    }

    let mut tokens = Vec::new();

    for (module_name, module_tests) in modules {
        let mut module_tokens = Vec::new();

        for test in module_tests {
            // Determine the test attribute:
            //   - attr specified: use it as-is
            //   - async without attr: #[tokio::test]
            //   - sync without attr: #[test]
            let attr_str = if let Some(ref attr) = test.attr {
                attr.clone()
            } else if test.is_async {
                "tokio::test".to_string()
            } else {
                "test".to_string()
            };

            // #[attr_str]
            module_tokens.push(TokenTree::Punct(Punct::new('#', Spacing::Alone)));
            module_tokens.push(TokenTree::Group(Group::new(
                Delimiter::Bracket,
                attr_str.parse::<TokenStream>().unwrap_or_else(|e| {
                    panic!("invalid attribute '{}': {}", attr_str, e);
                }),
            )));

            // #[ignore = "reason"] when ignore_if_env / ignore_unless_env apply.
            // Emitted after the test attribute so it composes with #[test] and
            // #[tokio::test] (both honor #[ignore]).
            if let Some(reason) = &test.ignore_reason {
                module_tokens.push(TokenTree::Punct(Punct::new('#', Spacing::Alone)));
                module_tokens.push(TokenTree::Group(Group::new(
                    Delimiter::Bracket,
                    TokenStream::from_iter([
                        TokenTree::Ident(Ident::new("ignore", Span::call_site())),
                        TokenTree::Punct(Punct::new('=', Spacing::Alone)),
                        TokenTree::Literal(Literal::string(reason)),
                    ]),
                )));
            }

            // async fn test_NAME() or fn test_NAME()
            if test.is_async {
                module_tokens.push(TokenTree::Ident(Ident::new("async", Span::call_site())));
            }
            module_tokens.push(TokenTree::Ident(Ident::new("fn", Span::call_site())));
            module_tokens.push(TokenTree::Ident(Ident::new(
                &test.test_name,
                Span::call_site(),
            )));
            module_tokens.push(TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::new(),
            )));

            // Build the function body:
            //   let __content = std::fs::read_to_string("PATH")
            //       .expect("failed to read fixture file: PATH");
            //   super::test_fn(Path::new("PATH"), &__content)
            let mut body_tokens: Vec<TokenTree> = vec![
                // let __content = std::fs::read_to_string("PATH").expect("...");
                TokenTree::Ident(Ident::new("let", Span::call_site())),
                TokenTree::Ident(Ident::new("__content", Span::call_site())),
                TokenTree::Punct(Punct::new('=', Spacing::Alone)),
                TokenTree::Ident(Ident::new("std", Span::call_site())),
                TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                TokenTree::Ident(Ident::new("fs", Span::call_site())),
                TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                TokenTree::Ident(Ident::new("read_to_string", Span::call_site())),
                TokenTree::Group(Group::new(
                    Delimiter::Parenthesis,
                    TokenStream::from_iter([TokenTree::Literal(Literal::string(&test.path))]),
                )),
                TokenTree::Punct(Punct::new('.', Spacing::Alone)),
                TokenTree::Ident(Ident::new("expect", Span::call_site())),
                TokenTree::Group(Group::new(
                    Delimiter::Parenthesis,
                    TokenStream::from_iter([TokenTree::Literal(Literal::string(&format!(
                        "failed to read fixture file: {}",
                        &test.path
                    )))]),
                )),
                TokenTree::Punct(Punct::new(';', Spacing::Alone)),
                // super::test_fn(std::path::Path::new("PATH"), &__content)
                TokenTree::Ident(Ident::new("super", Span::call_site())),
                TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                TokenTree::Ident(Ident::new(&test.test_fn, Span::call_site())),
                TokenTree::Group(Group::new(
                    Delimiter::Parenthesis,
                    TokenStream::from_iter([
                        // First arg: std::path::Path::new("PATH")
                        TokenTree::Ident(Ident::new("std", Span::call_site())),
                        TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                        TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                        TokenTree::Ident(Ident::new("path", Span::call_site())),
                        TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                        TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                        TokenTree::Ident(Ident::new("Path", Span::call_site())),
                        TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                        TokenTree::Punct(Punct::new(':', Spacing::Alone)),
                        TokenTree::Ident(Ident::new("new", Span::call_site())),
                        TokenTree::Group(Group::new(
                            Delimiter::Parenthesis,
                            TokenStream::from_iter([TokenTree::Literal(Literal::string(
                                &test.path,
                            ))]),
                        )),
                        // Comma separator
                        TokenTree::Punct(Punct::new(',', Spacing::Alone)),
                        // Second arg: &__content
                        TokenTree::Punct(Punct::new('&', Spacing::Alone)),
                        TokenTree::Ident(Ident::new("__content", Span::call_site())),
                    ]),
                )),
            ];

            // .await (for async tests, before .unwrap())
            if test.is_async {
                body_tokens.push(TokenTree::Punct(Punct::new('.', Spacing::Alone)));
                body_tokens.push(TokenTree::Ident(Ident::new("await", Span::call_site())));
            }

            // .unwrap();
            body_tokens.push(TokenTree::Punct(Punct::new('.', Spacing::Alone)));
            body_tokens.push(TokenTree::Ident(Ident::new("unwrap", Span::call_site())));
            body_tokens.push(TokenTree::Group(Group::new(
                Delimiter::Parenthesis,
                TokenStream::new(),
            )));
            body_tokens.push(TokenTree::Punct(Punct::new(';', Spacing::Alone)));

            module_tokens.push(TokenTree::Group(Group::new(
                Delimiter::Brace,
                TokenStream::from_iter(body_tokens),
            )));
        }

        if module_name.is_empty() {
            // No module wrapper for empty module name
            tokens.extend(module_tokens);
        } else {
            // mod module_name { ... }
            tokens.push(TokenTree::Ident(Ident::new("mod", Span::call_site())));
            tokens.push(TokenTree::Ident(Ident::new(
                &module_name,
                Span::call_site(),
            )));
            tokens.push(TokenTree::Group(Group::new(
                Delimiter::Brace,
                TokenStream::from_iter(module_tokens),
            )));
        }
    }

    TokenStream::from_iter(tokens)
}
