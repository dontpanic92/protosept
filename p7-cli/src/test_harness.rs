use p7::{
    InMemoryModuleProvider,
    ast::{Attribute, Expression},
    errors::Proto7Error,
    interpreter::context::Data as P7Value,
    semantic::SymbolKind,
    RunOptions,
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
// Test attribute struct for marking test functions
pub struct test(
    pub expected_type: string,
    pub expected_value: string,
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
}

#[derive(Debug)]
pub enum TestResult {
    Success,
    Failure(FailureReason),
}

#[derive(Debug)]
struct TestCase {
    function_name: String,
    expected_type: String,
    expected_value: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TestSummary {
    pub passed: usize,
    pub failed: usize,
}

fn extract_string_from_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.clone()),
        _ => None,
    }
}

fn parse_test_attribute(attr: &Attribute) -> Option<(String, String)> {
    if attr.name.name != "test" {
        return None;
    }

    let mut expected_type = None;
    let mut expected_value = None;

    for (name_opt, expr) in &attr.arguments {
        if let Some(name) = name_opt {
            match name.name.as_str() {
                "expected_type" => {
                    expected_type = extract_string_from_expression(expr);
                }
                "expected_value" => {
                    expected_value = extract_string_from_expression(expr);
                }
                _ => {}
            }
        }
    }

    match (expected_type, expected_value) {
        (Some(t), Some(v)) => Some((t, v)),
        _ => None,
    }
}

fn find_test_cases(module: &p7::bytecode::Module) -> Vec<TestCase> {
    let mut test_cases = Vec::new();

    for symbol in &module.symbols {
        if let SymbolKind::Function { func_id, .. } = symbol.kind {
            if let Some(func) = module.functions.get(func_id as usize) {
                for attr in &func.attributes {
                    if let Some((expected_type, expected_value)) = parse_test_attribute(attr) {
                        test_cases.push(TestCase {
                            function_name: symbol.name.clone(),
                            expected_type,
                            expected_value,
                        });
                    }
                }
            }
        }
    }

    test_cases
}

fn run_test_case(
    module: p7::bytecode::Module,
    test_case: &TestCase,
    options: RunOptions,
) -> Result<TestResult, Proto7Error> {
    let disassembly = unp7::disassemble_module(&module);
    let p7_result = match p7::run_with_options(module, &test_case.function_name, options) {
        Ok(value) => value,
        Err(e) => {
            println!("Disassembly of the module before error:\n{}", disassembly);
            return Ok(TestResult::Failure(FailureReason::ExecutionError(e)));
        }
    };

    // Compare results
    let expected_type = &test_case.expected_type;
    let expected_value_str = &test_case.expected_value;

    let actual_type = match p7_result {
        P7Value::Int(_) => {
            if *expected_type == "bool" { "bool" } else { "int" }
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
            if *expected_type == "bool" {
                let expected_bool = match expected_value_str.as_str() {
                    "true" => 1i64,
                    "false" => 0i64,
                    _ => return Ok(TestResult::Failure(FailureReason::ValueMismatch {
                        expected: expected_value_str.clone(),
                        found: actual_val.to_string(),
                    })),
                };
                *actual_val == expected_bool
            } else {
                expected_value_str
                    .parse::<i64>()
                    .map_or(false, |expected_val| *actual_val == expected_val)
            }
        }
        P7Value::Float(actual_val) => expected_value_str
            .parse::<f64>()
            .map_or(false, |expected_val| {
                (actual_val - expected_val).abs() < 1e-9
            }),
        P7Value::String(actual_val) => actual_val == expected_value_str,
        P7Value::Array(elements) => {
            let formatted = format_array(elements);
            formatted == *expected_value_str
        }
        _ => {
            let actual_value = format!("{:?}", p7_result);
            actual_value == *expected_value_str
        }
    };

    if !is_match {
        println!("Disassembly of the module before error:\n{}", disassembly);
        let found = match p7_result {
            P7Value::Int(i) => i.to_string(),
            P7Value::Float(f) => f.to_string(),
            P7Value::String(s) => s.clone(),
            P7Value::Array(elements) => format_array(&elements),
            _ => format!("{:?}", p7_result),
        };
        return Ok(TestResult::Failure(FailureReason::ValueMismatch {
            expected: expected_value_str.clone(),
            found,
        }));
    }

    Ok(TestResult::Success)
}

fn format_array(elements: &[P7Value]) -> String {
    let items: Vec<String> = elements.iter().map(|e| match e {
        P7Value::Int(i) => i.to_string(),
        P7Value::Float(f) => f.to_string(),
        P7Value::String(s) => format!("\"{}\"", s),
        P7Value::Array(inner) => format_array(inner),
        _ => format!("{:?}", e),
    }).collect();
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
    let test_cases = find_test_cases(&module);

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

        match run_test_case(test_module, &test_case, RunOptions { script_dir: script_dir.clone() }) {
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

    if let Some(arg) = args.get(0) {
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
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.ends_with(".p7") {
                        files.push(path);
                    }
                }
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
