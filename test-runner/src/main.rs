use std::{fs, path::PathBuf};
use toml::Value;
use serde::Deserialize;

#[derive(Debug)]
enum FailureReason {
    NotValid,
}

enum TestResult {
    Success,
    Failure(FailureReason),
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
    if first_comment_start.is_none() ||  first_comment_end.is_none() {
        return Ok(TestResult::Failure(FailureReason::NotValid));
    }

    let start = first_comment_start.unwrap();
    let end = first_comment_end.unwrap();
    if end <= start {
        return Ok(TestResult::Failure(FailureReason::NotValid));
    }

    let testcase: TestConfig = {
        let toml_content = &content[start + 2..end];
        match toml::from_str::<Value>(toml_content) {
            Ok(value) => {
                println!("{:?}", value);
                match value.try_into() {
                    Ok(testcase) => testcase,
                    Err(e) => {
                        println!("Error deserializing TOML in {:?}: {}", file_path, e);
                        return Ok(TestResult::Failure(FailureReason::NotValid));
                    }
                }
            }
            Err(e) => {
                println!("Error parsing TOML in {:?}: {}", file_path, e);
                return Ok(TestResult::Failure(FailureReason::NotValid));
            }
        }
    };

    println!("Running test: {:?}", testcase);
    Ok(TestResult::Success)
}

fn main() -> std::io::Result<()> {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.exists() || !tests_dir.is_dir() {
        println!("'tests' directory not found.");
        return Ok(());
    }

    for entry in fs::read_dir(&tests_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".p7") {
                    println!("Running tests: {:?}", path);
                    let result = run_test(&path);
                    match result {
                        Ok(result) => {
                            match result {
                                TestResult::Success => {
                                    println!("Success");
                                }
                                TestResult::Failure(reason) => {
                                    println!("Failure: {:?}", reason);
                                }
                            }
                        }
                        Err(err) => {
                            println!("Error: {:?}", err)
                        }   
                    }
                }
            }
        }
    }

    Ok(())
}
