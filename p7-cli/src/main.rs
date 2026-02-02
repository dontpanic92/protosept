mod module_provider;
mod test_harness;

use module_provider::FileSystemModuleProvider;
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

fn print_help(program_name: &str) {
    println!(
        "Usage:\n  {program_name} <script.p7>\n  {program_name} test [test-file]\n\nNotes:\n  - `test-file` is optional and can be a path or a name under `tests/`.\n  - `p7 test --help` shows test-runner-style help."
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help(&args[0]);
        return ExitCode::SUCCESS;
    }

    if args.get(1).map(|s| s.as_str()) == Some("test") {
        let test_args: Vec<String> = args.into_iter().skip(2).collect();
        match test_harness::run_cli("p7 test", &test_args) {
            Ok(summary) => {
                if summary.failed > 0 {
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        }
    }

    if args.len() != 2 {
        eprintln!("Usage: {} <script.p7>\nTry: {} --help", args[0], args[0]);
        return ExitCode::from(1);
    }

    let script_path = Path::new(&args[1]);

    // Read the script file
    let contents = match fs::read_to_string(script_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading '{}': {}", script_path.display(), e);
            return ExitCode::from(1);
        }
    };

    // Create module provider with script's directory as base
    let provider = FileSystemModuleProvider::new(script_path);

    // Compile and run with "main" as entrypoint
    match p7::compile_and_run_with_provider(contents, "main", Box::new(provider)) {
        Ok(_result) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}
