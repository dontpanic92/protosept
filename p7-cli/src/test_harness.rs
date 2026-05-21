use p7::{
    InMemoryModuleProvider, RunOptions,
    ast::{Attribute, Expression},
    errors::Proto7Error,
    interpreter::context::Data as P7Value,
    semantic::SymbolKind,
};
use std::{fs, path::PathBuf};

use crate::module_provider::FileSystemModuleProvider;

/// Composite module provider: tries in-memory modules first, then falls back
/// to filesystem resolution (for std.* and other on-disk modules).
#[derive(Clone)]
struct CompositeModuleProvider {
    in_mem: InMemoryModuleProvider,
    fs: FileSystemModuleProvider,
}

impl CompositeModuleProvider {
    fn new(in_mem: InMemoryModuleProvider, test_file: &PathBuf) -> Self {
        Self {
            in_mem,
            fs: FileSystemModuleProvider::new(test_file.as_path()),
        }
    }
}

impl p7::ModuleProvider for CompositeModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        self.in_mem
            .load_module(module_path)
            .or_else(|| self.fs.load_module(module_path))
    }

    fn clone_boxed(&self) -> Box<dyn p7::ModuleProvider> {
        Box::new(self.clone())
    }
}

// Define the test modules that will be provided in-memory
const TEST_MODULE_SOURCE: &str = r#"
// Test attribute struct for marking test functions.
//
// Two mutually exclusive forms are recognised by the harness:
//   @test(expected_type = "...", expected_value = "...")
//   @test(runtime_error  = "<substring of Proto7Error Display>")
// All fields are nullable so any single form type-checks against the struct.
pub struct test(
    pub expected_type: ?string,
    pub expected_value: ?string,
    pub runtime_error:  ?string,
);

// Another struct to test selective import
pub struct test2(
    pub some_field: string,
);
"#;

// Parent module with symbol `bar`
const FOO_MODULE_SOURCE: &str = r#"
pub fn bar() -> int {
    return 7;
}
"#;

// Child module `foo.bar_mod`
const FOO_BAR_MOD_SOURCE: &str = r#"
pub fn value() -> int {
    return 5;
}
"#;

#[allow(dead_code)]
#[derive(Debug)]
pub enum FailureReason {
    NoTestFunctions,
    ExecutionError(Proto7Error),
    TypeMismatch { expected: String, found: String },
    ValueMismatch { expected: String, found: String },
    InvalidTestAttribute(String),
    CompileDidNotFail,
    /// `@test(runtime_error = ...)` was set but the function returned Ok.
    RuntimeErrorDidNotOccur {
        expected_substring: String,
        returned: String,
    },
    /// The function did return Err, but `Proto7Error::Display` did not contain
    /// the expected substring.
    RuntimeErrorMismatch {
        expected_substring: String,
        found: String,
    },
}

#[derive(Debug)]
pub enum TestResult {
    Success,
    Failure(FailureReason),
}

#[derive(Debug, Clone)]
enum TestExpectation {
    /// `@test(expected_type = ..., expected_value = ...)` — function must
    /// return Ok with a matching value.
    Returns {
        expected_type: String,
        expected_value: String,
    },
    /// `@test(runtime_error = ...)` — function must return Err and the
    /// formatted error must contain this substring. Empty substring matches
    /// any runtime error.
    RuntimeError { substring: String },
}

#[derive(Debug)]
struct TestCase {
    function_name: String,
    expectation: TestExpectation,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TestSummary {
    pub passed: usize,
    pub failed: usize,
}

fn extract_string_from_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.to_string()),
        _ => None,
    }
}

fn parse_test_attribute(attr: &Attribute) -> Result<Option<TestExpectation>, FailureReason> {
    if attr.name.name != "test" {
        return Ok(None);
    }

    let mut expected_type = None;
    let mut expected_value = None;
    let mut runtime_error = None;

    for (name_opt, expr) in &attr.arguments {
        let Some(name) = name_opt else {
            return Err(FailureReason::InvalidTestAttribute(
                "@test arguments must be named".to_string(),
            ));
        };

        let Some(value) = extract_string_from_expression(expr) else {
            return Err(FailureReason::InvalidTestAttribute(format!(
                "@test argument '{}' must be a string literal",
                name.name
            )));
        };

        match name.name.as_str() {
            "expected_type" => {
                if expected_type.replace(value).is_some() {
                    return Err(FailureReason::InvalidTestAttribute(
                        "Duplicate @test argument 'expected_type'".to_string(),
                    ));
                }
            }
            "expected_value" => {
                if expected_value.replace(value).is_some() {
                    return Err(FailureReason::InvalidTestAttribute(
                        "Duplicate @test argument 'expected_value'".to_string(),
                    ));
                }
            }
            "runtime_error" => {
                if runtime_error.replace(value).is_some() {
                    return Err(FailureReason::InvalidTestAttribute(
                        "Duplicate @test argument 'runtime_error'".to_string(),
                    ));
                }
            }
            _ => {
                return Err(FailureReason::InvalidTestAttribute(format!(
                    "Unknown @test argument '{}'",
                    name.name
                )));
            }
        }
    }

    let returns_form = expected_type.is_some() || expected_value.is_some();
    let error_form = runtime_error.is_some();

    if returns_form && error_form {
        return Err(FailureReason::InvalidTestAttribute(
            "@test arguments 'runtime_error' and 'expected_type'/'expected_value' are mutually exclusive".to_string(),
        ));
    }

    if let Some(substring) = runtime_error {
        return Ok(Some(TestExpectation::RuntimeError { substring }));
    }

    match (expected_type, expected_value) {
        (Some(t), Some(v)) => Ok(Some(TestExpectation::Returns {
            expected_type: t,
            expected_value: v,
        })),
        (None, None) => Err(FailureReason::InvalidTestAttribute(
            "@test requires either 'runtime_error' or both 'expected_type' and 'expected_value'"
                .to_string(),
        )),
        (None, _) => Err(FailureReason::InvalidTestAttribute(
            "Missing @test argument 'expected_type'".to_string(),
        )),
        (_, None) => Err(FailureReason::InvalidTestAttribute(
            "Missing @test argument 'expected_value'".to_string(),
        )),
    }
}

fn find_test_cases(module: &p7::bytecode::Module) -> Result<Vec<TestCase>, FailureReason> {
    let mut test_cases = Vec::new();

    for symbol in &module.symbols {
        if let SymbolKind::Function { func_id, .. } = symbol.kind
            && let Some(func) = module.functions.get(func_id as usize)
        {
            for attr in &func.attributes {
                if let Some(expectation) = parse_test_attribute(attr)? {
                    test_cases.push(TestCase {
                        function_name: symbol.name.to_string(),
                        expectation,
                    });
                }
            }
        }
    }

    Ok(test_cases)
}

fn run_test_case(
    module: p7::bytecode::Module,
    test_case: &TestCase,
    options: RunOptions,
) -> Result<TestResult, Proto7Error> {
    let disassembly = unp7::disassemble_module(&module);
    let run_result = p7::run_with_options(module, &test_case.function_name, options);

    match &test_case.expectation {
        TestExpectation::RuntimeError { substring } => match run_result {
            Ok(value) => {
                let returned = format_value(&value);
                Ok(TestResult::Failure(FailureReason::RuntimeErrorDidNotOccur {
                    expected_substring: substring.clone(),
                    returned,
                }))
            }
            Err(err) => {
                let rendered = format!("{}", err);
                if substring.is_empty() || rendered.contains(substring) {
                    Ok(TestResult::Success)
                } else {
                    println!("Disassembly of the module before error:\n{}", disassembly);
                    Ok(TestResult::Failure(FailureReason::RuntimeErrorMismatch {
                        expected_substring: substring.clone(),
                        found: rendered,
                    }))
                }
            }
        },
        TestExpectation::Returns {
            expected_type,
            expected_value,
        } => {
            let p7_result = match run_result {
                Ok(value) => value,
                Err(e) => {
                    println!("Disassembly of the module before error:\n{}", disassembly);
                    return Ok(TestResult::Failure(FailureReason::ExecutionError(e)));
                }
            };

            let actual_type = match p7_result {
                P7Value::Int(_) => {
                    if expected_type == "bool" {
                        "bool"
                    } else {
                        "int"
                    }
                }
                P7Value::Float(_) => "float",
                P7Value::String(_) => "string",
                P7Value::Array(_) => "array",
                _ => "unknown",
            }
            .to_string();

            if actual_type != *expected_type {
                return Ok(TestResult::Failure(FailureReason::TypeMismatch {
                    expected: expected_type.clone(),
                    found: actual_type,
                }));
            }

            let is_match = match &p7_result {
                P7Value::Int(actual_val) => {
                    if expected_type == "bool" {
                        let expected_bool = match expected_value.as_str() {
                            "true" => 1i64,
                            "false" => 0i64,
                            _ => {
                                return Ok(TestResult::Failure(FailureReason::ValueMismatch {
                                    expected: expected_value.clone(),
                                    found: actual_val.to_string(),
                                }));
                            }
                        };
                        *actual_val == expected_bool
                    } else {
                        expected_value.parse::<i64>() == Ok(*actual_val)
                    }
                }
                P7Value::Float(actual_val) => expected_value
                    .parse::<f64>()
                    .is_ok_and(|expected_val| (actual_val - expected_val).abs() < 1e-9),
                P7Value::String(actual_val) => {
                    actual_val.as_ref() == expected_value.as_str()
                }
                P7Value::Array(elements) => {
                    let formatted = format_array(elements);
                    formatted == *expected_value
                }
                _ => {
                    let actual_value = format!("{:?}", p7_result);
                    actual_value == *expected_value
                }
            };

            if !is_match {
                println!("Disassembly of the module before error:\n{}", disassembly);
                let found = format_value(&p7_result);
                return Ok(TestResult::Failure(FailureReason::ValueMismatch {
                    expected: expected_value.clone(),
                    found,
                }));
            }

            Ok(TestResult::Success)
        }
    }
}

fn format_value(value: &P7Value) -> String {
    match value {
        P7Value::Int(i) => i.to_string(),
        P7Value::Float(f) => f.to_string(),
        P7Value::String(s) => s.to_string(),
        P7Value::Array(elements) => format_array(elements),
        other => format!("{:?}", other),
    }
}

fn format_array(elements: &[P7Value]) -> String {
    let items: Vec<String> = elements
        .iter()
        .map(|e| match e {
            P7Value::Int(i) => i.to_string(),
            P7Value::Float(f) => f.to_string(),
            P7Value::String(s) => format!("\"{}\"", s),
            P7Value::Array(inner) => format_array(inner),
            _ => format!("{:?}", e),
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn run_tests_in_file(file_path: &PathBuf) -> anyhow::Result<Vec<(String, TestResult)>> {
    let content = fs::read_to_string(file_path)?;

    // Compute the script directory for __script_dir__
    let script_dir = file_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .or_else(|| file_path.parent().map(|p| p.to_path_buf()))
        .map(|p| p.to_string_lossy().into_owned());

    // Create a composite module provider: in-memory test modules + filesystem std
    let mut in_mem = InMemoryModuleProvider::new();
    in_mem.add_module("test".to_string(), TEST_MODULE_SOURCE.to_string());
    in_mem.add_module("foo".to_string(), FOO_MODULE_SOURCE.to_string());
    in_mem.add_module("foo.bar_mod".to_string(), FOO_BAR_MOD_SOURCE.to_string());
    let module_provider = CompositeModuleProvider::new(in_mem, file_path);

    // Compile-fail tests: add `// compile_fail` anywhere in the file.
    if content
        .lines()
        .any(|l| l.trim_start().starts_with("// compile_fail"))
    {
        match p7::compile_with_provider(content.clone(), Box::new(module_provider.clone())) {
            Ok(_) => {
                return Ok(vec![(
                    "compile_fail".to_string(),
                    TestResult::Failure(FailureReason::CompileDidNotFail),
                )]);
            }
            Err(_) => {
                return Ok(vec![("compile_fail".to_string(), TestResult::Success)]);
            }
        }
    }

    // Compile the p7 code with the module provider
    let module = match p7::compile_with_provider(content.clone(), Box::new(module_provider.clone()))
    {
        Ok(m) => m,
        Err(e) => {
            return Ok(vec![(
                "compile".to_string(),
                TestResult::Failure(FailureReason::ExecutionError(e)),
            )]);
        }
    };

    // Find all test cases
    let test_cases = match find_test_cases(&module) {
        Ok(test_cases) => test_cases,
        Err(reason) => {
            return Ok(vec![(
                "test-attribute".to_string(),
                TestResult::Failure(reason),
            )]);
        }
    };

    if test_cases.is_empty() {
        return Ok(vec![(
            "no-tests".to_string(),
            TestResult::Failure(FailureReason::NoTestFunctions),
        )]);
    }

    let mut results = Vec::new();
    for test_case in test_cases {
        // Reuse the already-compiled module (clone is cheap compared to recompilation)
        let test_module = module.clone();

        match run_test_case(
            test_module,
            &test_case,
            RunOptions {
                script_dir: script_dir.clone(),
            },
        ) {
            Ok(result) => results.push((test_case.function_name.clone(), result)),
            Err(e) => results.push((
                test_case.function_name.clone(),
                TestResult::Failure(FailureReason::ExecutionError(e)),
            )),
        }
    }

    Ok(results)
}

pub fn print_help(program_name: &str) {
    println!(
        "Usage: {program_name} [test-file]\n  test-file: optional path or file name (with or without .p7) under 'tests/'"
    );
}

/// Runs the test harness similarly to the historical `test-runner` binary.
///
/// - `program_name`: string used for help/usage output (e.g. "p7 test" or "test-runner").
/// - `args`: arguments *after* the `test` subcommand (or after the program name for `test-runner`).
/// - Prints per-test output and summary.
/// - Returns a `TestSummary`.
pub fn run_cli(program_name: &str, args: &[String]) -> anyhow::Result<TestSummary> {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.exists() || !tests_dir.is_dir() {
        println!("'tests' directory not found.");
        return Ok(TestSummary::default());
    }

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help(program_name);
        return Ok(TestSummary::default());
    }

    let mut files: Vec<PathBuf> = Vec::new();

    if let Some(arg) = args.first() {
        let direct = PathBuf::from(arg);
        let candidates = [
            direct.clone(),
            tests_dir.join(arg),
            tests_dir.join(format!("{}.p7", arg)),
        ];
        let found = candidates
            .iter()
            .find(|p| p.exists() && p.is_file())
            .cloned();

        match found {
            Some(path) => files.push(path),
            None => {
                println!(
                    "Test file '{}' not found. Provide a valid path or file name under {:?}.",
                    arg, tests_dir
                );
                anyhow::bail!("test file not found");
            }
        }
    } else {
        for entry in fs::read_dir(&tests_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && file_name.ends_with(".p7")
            {
                files.push(path);
            }
        }
    }

    if files.is_empty() {
        println!("No test files found to run.");
        return Ok(TestSummary::default());
    }

    let mut summary = TestSummary::default();

    for path in files {
        match run_tests_in_file(&path) {
            Ok(results) => {
                for (test_name, result) in results {
                    print!("Running test: {:?}::{} ... ", path, test_name);
                    match result {
                        TestResult::Success => {
                            println!("OK");
                            summary.passed += 1;
                        }
                        TestResult::Failure(reason) => {
                            println!("FAILED: {:?}", reason);
                            summary.failed += 1;
                        }
                    }
                }
            }
            Err(err) => {
                println!("ERROR loading {:?}: {:?}", path, err);
                summary.failed += 1;
            }
        }
    }

    println!(
        "\nTest results: {} passed, {} failed.",
        summary.passed, summary.failed
    );

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p7::ast::Identifier;
    use p7::intern::InternedString;

    fn ident(name: &str) -> Identifier {
        Identifier {
            name: InternedString::from(name),
            line: 1,
            col: 1,
        }
    }

    fn test_attr(arguments: Vec<(Option<Identifier>, Expression)>) -> Attribute {
        Attribute {
            name: ident("test"),
            arguments,
        }
    }

    #[test]
    fn test_attribute_accepts_required_string_keys() {
        let attr = test_attr(vec![
            (
                Some(ident("expected_type")),
                Expression::StringLiteral(InternedString::from("int")),
            ),
            (
                Some(ident("expected_value")),
                Expression::StringLiteral(InternedString::from("42")),
            ),
        ]);

        let parsed = parse_test_attribute(&attr).unwrap();
        assert!(matches!(
            parsed,
            Some(TestExpectation::Returns { ref expected_type, ref expected_value })
                if expected_type == "int" && expected_value == "42"
        ));
    }

    #[test]
    fn test_attribute_rejects_unknown_key() {
        let attr = test_attr(vec![
            (
                Some(ident("expected_type")),
                Expression::StringLiteral(InternedString::from("int")),
            ),
            (
                Some(ident("unexpected")),
                Expression::StringLiteral(InternedString::from("42")),
            ),
            (
                Some(ident("expected_value")),
                Expression::StringLiteral(InternedString::from("42")),
            ),
        ]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("Unknown @test argument 'unexpected'")
        ));
    }

    #[test]
    fn test_attribute_rejects_non_string_value() {
        let attr = test_attr(vec![
            (
                Some(ident("expected_type")),
                Expression::StringLiteral(InternedString::from("int")),
            ),
            (
                Some(ident("expected_value")),
                Expression::IntegerLiteral(42),
            ),
        ]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("must be a string literal")
        ));
    }

    #[test]
    fn test_attribute_accepts_runtime_error_alone() {
        let attr = test_attr(vec![(
            Some(ident("runtime_error")),
            Expression::StringLiteral(InternedString::from("out of bounds")),
        )]);

        assert!(matches!(
            parse_test_attribute(&attr).unwrap(),
            Some(TestExpectation::RuntimeError { ref substring })
                if substring == "out of bounds"
        ));
    }

    #[test]
    fn test_attribute_accepts_runtime_error_empty_string_as_catchall() {
        let attr = test_attr(vec![(
            Some(ident("runtime_error")),
            Expression::StringLiteral(InternedString::from("")),
        )]);

        assert!(matches!(
            parse_test_attribute(&attr).unwrap(),
            Some(TestExpectation::RuntimeError { ref substring })
                if substring.is_empty()
        ));
    }

    #[test]
    fn test_attribute_rejects_mixing_runtime_error_with_returns_form() {
        let attr = test_attr(vec![
            (
                Some(ident("expected_type")),
                Expression::StringLiteral(InternedString::from("int")),
            ),
            (
                Some(ident("runtime_error")),
                Expression::StringLiteral(InternedString::from("oops")),
            ),
        ]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("mutually exclusive")
        ));
    }

    #[test]
    fn test_attribute_rejects_duplicate_runtime_error() {
        let attr = test_attr(vec![
            (
                Some(ident("runtime_error")),
                Expression::StringLiteral(InternedString::from("first")),
            ),
            (
                Some(ident("runtime_error")),
                Expression::StringLiteral(InternedString::from("second")),
            ),
        ]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("Duplicate @test argument 'runtime_error'")
        ));
    }

    #[test]
    fn test_attribute_rejects_runtime_error_non_string_literal() {
        let attr = test_attr(vec![(
            Some(ident("runtime_error")),
            Expression::IntegerLiteral(7),
        )]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("must be a string literal")
        ));
    }

    #[test]
    fn test_attribute_rejects_empty_attribute() {
        let attr = test_attr(vec![]);

        assert!(matches!(
            parse_test_attribute(&attr),
            Err(FailureReason::InvalidTestAttribute(message))
                if message.contains("either 'runtime_error'")
        ));
    }

    /// End-to-end harness coverage for the runtime_error variant: compile a
    /// tiny in-memory file, run it through `run_tests_in_file`, and assert on
    /// the resulting per-function outcome.
    fn run_inline(src: &str) -> Vec<(String, TestResult)> {
        use std::io::Write;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "p7_runtime_error_inline_{}_{}.p7",
            std::process::id(),
            n
        ));
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(src.as_bytes()).expect("write");
        drop(f);
        let results = super::run_tests_in_file(&path).expect("run");
        let _ = std::fs::remove_file(&path);
        results
    }

    #[test]
    fn runtime_error_substring_match_passes() {
        let results = run_inline(
            r#"
            import test.test;

            @test(runtime_error = "out of bounds")
            pub fn oob() -> int {
                let arr = [1, 2, 3];
                return arr[7];
            }
        "#,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0].1, TestResult::Success),
            "expected Success, got {:?}",
            results[0].1
        );
    }

    #[test]
    fn runtime_error_substring_mismatch_fails() {
        let results = run_inline(
            r#"
            import test.test;

            @test(runtime_error = "totally not the real message")
            pub fn oob() -> int {
                let arr = [1, 2, 3];
                return arr[7];
            }
        "#,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].1,
                TestResult::Failure(FailureReason::RuntimeErrorMismatch { .. })
            ),
            "expected RuntimeErrorMismatch, got {:?}",
            results[0].1
        );
    }

    #[test]
    fn runtime_error_did_not_occur_fails() {
        let results = run_inline(
            r#"
            import test.test;

            @test(runtime_error = "something")
            pub fn ok_path() -> int {
                return 1;
            }
        "#,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].1,
                TestResult::Failure(FailureReason::RuntimeErrorDidNotOccur { .. })
            ),
            "expected RuntimeErrorDidNotOccur, got {:?}",
            results[0].1
        );
    }

    #[test]
    fn runtime_error_empty_substring_catches_any_error() {
        let results = run_inline(
            r#"
            import test.test;

            @test(runtime_error = "")
            pub fn oob() -> int {
                let arr = [1, 2, 3];
                return arr[7];
            }
        "#,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0].1, TestResult::Success),
            "expected Success, got {:?}",
            results[0].1
        );
    }
}
