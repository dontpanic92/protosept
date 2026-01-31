//! Test runner for the p7 language.
//! Usage: test-runner [test-file]
//!   test-file: optional path or file name (with or without .p7) under `tests/`.

use p7::{
    InMemoryModuleProvider,
    ast::{Attribute, Expression},
    errors::Proto7Error,
    interpreter::context::Data as P7Value,
    semantic::SymbolKind,
};
use std::{env, fs, path::PathBuf};

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

#[derive(Debug)]
enum FailureReason {
    NoTestFunctions,
    ExecutionError(Proto7Error),
    TypeMismatch { expected: String, found: String },
    ValueMismatch { expected: String, found: String },
    InvalidTestAttribute(String),
    CompileDidNotFail,
}

#[derive(Debug)]
enum TestResult {
    Success,
    Failure(FailureReason),
}

#[derive(Debug)]
struct TestCase {
    function_name: String,
    expected_type: String,
    expected_value: String,
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
) -> Result<TestResult, Proto7Error> {
    let disassembly = unp7::disassemble_module(&module);
    let p7_result = match p7::run(module, &test_case.function_name) {
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
        P7Value::Int(_) => "int",
        P7Value::Float(_) => "float",
        P7Value::String(_) => "string",
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
        P7Value::Int(actual_val) => expected_value_str
            .parse::<i32>()
            .map_or(false, |expected_val| *actual_val == expected_val),
        P7Value::Float(actual_val) => expected_value_str
            .parse::<f64>()
            .map_or(false, |expected_val| {
                (actual_val - expected_val).abs() < 1e-9
            }),
        P7Value::String(actual_val) => actual_val == expected_value_str,
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
            _ => format!("{:?}", p7_result),
        };
        return Ok(TestResult::Failure(FailureReason::ValueMismatch {
            expected: expected_value_str.clone(),
            found,
        }));
    }

    Ok(TestResult::Success)
}

fn run_tests_in_file(file_path: &PathBuf) -> anyhow::Result<Vec<(String, TestResult)>> {
    let content = fs::read_to_string(file_path)?;

    // Create module provider with the test module
    // The module is registered as "test" which contains symbols like "test" and "test2"
    let mut module_provider = InMemoryModuleProvider::new();
    module_provider.add_module("test".to_string(), TEST_MODULE_SOURCE.to_string());
    module_provider.add_module("foo".to_string(), FOO_MODULE_SOURCE.to_string());
    module_provider.add_module("foo.bar_mod".to_string(), FOO_BAR_MOD_SOURCE.to_string());

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
        // Clone module for each test
        let test_module = match p7::compile_with_provider(
            content.as_str().to_string(),
            Box::new(module_provider.clone()),
        ) {
            Ok(m) => m,
            Err(e) => {
                results.push((
                    test_case.function_name.clone(),
                    TestResult::Failure(FailureReason::ExecutionError(e)),
                ));
                continue;
            }
        };

        match run_test_case(test_module, &test_case) {
            Ok(result) => results.push((test_case.function_name.clone(), result)),
            Err(e) => results.push((
                test_case.function_name.clone(),
                TestResult::Failure(FailureReason::ExecutionError(e)),
            )),
        }
    }

    Ok(results)
}

fn main() -> std::io::Result<()> {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.exists() || !tests_dir.is_dir() {
        println!("'tests' directory not found.");
        return Ok(());
    }

    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: test-runner [test-file]\n  test-file: optional path or file name (with or without .p7) under 'tests/'"
        );
        return Ok(());
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
                std::process::exit(1);
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
        return Ok(());
    }

    let mut passed_count = 0;
    let mut failed_count = 0;

    for path in files {
        match run_tests_in_file(&path) {
            Ok(results) => {
                for (test_name, result) in results {
                    print!("Running test: {:?}::{} ... ", path, test_name);
                    match result {
                        TestResult::Success => {
                            println!("OK");
                            passed_count += 1;
                        }
                        TestResult::Failure(reason) => {
                            println!("FAILED: {:?}", reason);
                            failed_count += 1;
                        }
                    }
                }
            }
            Err(err) => {
                println!("ERROR loading {:?}: {:?}", path, err);
                failed_count += 1;
            }
        }
    }

    println!(
        "
Test results: {} passed, {} failed.",
        passed_count, failed_count
    );

    if failed_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}
