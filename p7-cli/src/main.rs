mod module_provider;

use module_provider::FileSystemModuleProvider;
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: p7 <script.p7>");
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
