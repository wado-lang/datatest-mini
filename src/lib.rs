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

        // Use test function name as module name
        collect_matching_files(
            &full_path,
            &full_path,
            &regex,
            &entry.test_fn,
            entry.is_async,
            &entry.attr,
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

struct TestEntry {
    module_name: String,
    test_name: String,
    path: String,
    test_fn: String,
    is_async: bool,
    attr: Option<String>,
}

fn collect_matching_files(
    base_path: &Path,
    current_path: &Path,
    pattern: &Regex,
    test_fn: &str,
    is_async: bool,
    attr: &Option<String>,
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
            collect_matching_files(base_path, &path, pattern, test_fn, is_async, attr, tests);
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
        });
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
