use std::{fs, path::PathBuf};
use toml::Value;
use serde::Deserialize;
use p7::interpreter::context::Data as P7Value;

#[derive(Debug)]
enum FailureReason {
    NotValidToml,
    InvalidTomlFormat(String),
    NoTestConfig,
    ExecutionError(String),
    TypeMismatch {
        expected: String,
        found: String,
    },
    ValueMismatch {
        expected: String,
        found: String,
    },
}

#[derive(Debug)]
enum TestResult {
    Success,
    Failure(FailureReason),
    Error(String), // For unexpected errors during test execution
}

#[derive(Deserialize, Debug)]
struct TestCase {
    entrypoint: String,
    expected_type: String,
    expected_value: String,
}

#[derive(Deserialize, Debug)]
struct TestConfig {
    testcase: TestCase,
}

fn run_test(file_path: &PathBuf) -> anyhow::Result<TestResult> {
    let content = fs::read_to_string(file_path)?;
    let first_comment_start = content.find("/*");
    let first_comment_end = content.find("*/");

    let test_config: TestConfig = if let (Some(start), Some(end)) = (first_comment_start, first_comment_end) {
        if end <= start {
            return Ok(TestResult::Failure(FailureReason::NotValidToml));
        }
        let toml_content = &content[start + 2..end];
        match toml::from_str::<Value>(toml_content) {
            Ok(value) => {
                match value.try_into() {
                    Ok(config) => config,
                    Err(e) => return Ok(TestResult::Failure(FailureReason::InvalidTomlFormat(e.to_string()))),
                }
            }
            Err(e) => return Ok(TestResult::Failure(FailureReason::InvalidTomlFormat(e.to_string()))),
        }
    } else {
        return Ok(TestResult::Failure(FailureReason::NoTestConfig));
    };

    // Execute the p7 code
    let entrypoint = &test_config.testcase.entrypoint;
    let p7_result = match p7::run_p7_code(content, entrypoint) {
        Ok(value) => value,
        Err(e) => return Ok(TestResult::Failure(FailureReason::ExecutionError(e.to_string()))),
    };

    // Compare results
    let expected_type = &test_config.testcase.expected_type;
    let expected_value_str = &test_config.testcase.expected_value;

    let actual_type = match p7_result {
        P7Value::Int(_) => "int",
        P7Value::Float(_) => "float",
        // P7Value::Bool(_) => "bool",
        // P7Value::String(_) => "string",
        // P7Value::Void => "void",
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
        // P7Value::Bool(b) => ("bool".to_string(), b.to_string()),
        // P7Value::String(s) => ("string".to_string(), s),
        // P7Value::Void => ("void".to_string(), "void".to_string()),
        _ => {
            let actual_value = format!("{:?}", p7_result);
            actual_value == *expected_value_str
        }
    };

    if !is_match {
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
                    print!("Running test: {:?} ... ", path);
                    match run_test(&path) {
                        Ok(result) => {
                            match result {
                                TestResult::Success => {
                                    println!("OK");
                                    passed_count += 1;
                                }
                                TestResult::Failure(reason) => {
                                    println!("FAILED: {:?}", reason);
                                    failed_count += 1;
                                }
                                TestResult::Error(e) => {
                                    println!("ERROR: {}", e);
                                    failed_count += 1;
                                }
                            }
                        }
                        Err(err) => {
                            println!("ERROR: {:?}", err);
                            failed_count += 1;
                        }
                    }
                }
            }
        }
    }

    println!("
Test results: {} passed, {} failed.", passed_count, failed_count);

    Ok(())
}