use p7::{
    ast::{Attribute, Expression},
    errors::Proto7Error,
    interpreter::context::Data as P7Value,
    semantic::{UserDefinedType, SymbolKind},
};
use std::{fs, path::PathBuf};

#[derive(Debug)]
enum FailureReason {
    NoTestFunctions,
    ExecutionError(Proto7Error),
    TypeMismatch { expected: String, found: String },
    ValueMismatch { expected: String, found: String },
    InvalidTestAttribute(String),
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
        if let SymbolKind::Function { type_id, .. } = symbol.kind {
            if let UserDefinedType::Function(func) = &module.types[type_id as usize] {
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
        _ => "unknown",
    }
    .to_string();

    if actual_type != *expected_type {
        return Ok(TestResult::Failure(FailureReason::TypeMismatch {
            expected: expected_type.clone(),
            found: actual_type,
        }));
    }

    let is_match = match p7_result {
        P7Value::Int(actual_val) => expected_value_str
            .parse::<i32>()
            .map_or(false, |expected_val| actual_val == expected_val),
        P7Value::Float(actual_val) => expected_value_str
            .parse::<f64>()
            .map_or(false, |expected_val| (actual_val - expected_val).abs() < 1e-9),
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

    // Compile the p7 code
    let module = match p7::compile(content.clone()) {
        Ok(m) => m,
        Err(e) => {
            return Ok(vec![(
                "compile".to_string(),
                TestResult::Failure(FailureReason::ExecutionError(e)),
            )])
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
        let test_module = match p7::compile(content.as_str().to_string()) {
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

    let mut passed_count = 0;
    let mut failed_count = 0;

    for entry in fs::read_dir(&tests_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".p7") {
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
