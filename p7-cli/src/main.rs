mod dispatch;
mod module_provider;
mod repl;
mod test_harness;

use dispatch::{DispatchMode, Subcommand};
use module_provider::FileSystemModuleProvider;
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

#[derive(Debug, Default, Clone, Copy)]
struct GlobalOptions {
    help: bool,
    verbose: bool,
}

fn print_help(program_name: &str) {
    println!(
        "Usage:\n  {program_name} [global-options] <script.p7> [script-args...]\n  {program_name} [global-options] <subcommand> [args...]\n  {program_name} [global-options]\n\nSubcommands:\n  run <script.p7> [-- script-args...]\n  test [test-file]\n  repl\n  version\n  help\n\nGlobal options:\n  -h, --help      Show help\n  -v, --verbose   Verbose output (currently a no-op)\n\nNotes:\n  - Direct script mode forwards all tokens after the script path verbatim.\n  - `p7 foo.p7 --help` must not trigger p7 help; it is forwarded to the script.\n  - `p7 run foo.p7 -- -n 1` uses `--` to forward args in subcommand mode."
    );
}

fn print_run_help(program_name: &str) {
    println!(
        "Usage:\n  {program_name} run <script.p7> [-- script-args...]\n\nNotes:\n  - In `run`, script args MUST come after `--`.\n  - Flags like `-n` before `--` are treated as `run` options and error if unknown."
    );
}

fn print_repl_help(program_name: &str) {
    println!("Usage:\n  {program_name} repl\n\nStarts the REPL.");
}

fn parse_global_options(global_argv: &[String]) -> Result<GlobalOptions, String> {
    let mut opts = GlobalOptions::default();

    for a in global_argv.iter().skip(1) {
        match a.as_str() {
            "-h" | "--help" => opts.help = true,
            "-v" | "--verbose" => opts.verbose = true,
            _ if a.starts_with('-') && a != "-" => {
                return Err(format!("Unknown global option: {a}"));
            }
            _ => {}
        }
    }

    Ok(opts)
}

fn read_script(script_path: &Path) -> Result<String, String> {
    fs::read_to_string(script_path)
        .map_err(|e| format!("Error reading '{}': {e}", script_path.display()))
}

fn run_script(script_path: &Path, _script_args: &[String]) -> Result<(), String> {
    let contents = read_script(script_path)?;
    let provider = FileSystemModuleProvider::new(script_path);

    // Compute the containing directory of the script for __script_dir__
    let script_dir = script_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .or_else(|| script_path.parent().map(|p| p.to_path_buf()))
        .map(|p| p.to_string_lossy().into_owned());

    let options = p7::RunOptions {
        script_dir,
    };

    p7::compile_and_run_with_provider_and_options(contents, "main", Box::new(provider), options)
        .map(|_result| ())
        .map_err(|e| format!("Error: {e}"))
}

fn parse_run_args(args: &[String]) -> Result<(String, Vec<String>), String> {
    if args.is_empty() {
        return Err("Missing <script.p7>".to_string());
    }

    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Err("__HELP__".to_string());
    }

    let dd = args.iter().position(|a| a == "--");
    let (pre, forwarded) = match dd {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (args, Vec::new()),
    };

    // First non-flag token in `pre` is the script path.
    let mut script_path: Option<String> = None;
    let mut seen_script = false;
    for a in pre {
        if a.starts_with('-') && a != "-" {
            // No known run options yet.
            return Err(format!("Unknown run option: {a}"));
        }
        if !seen_script {
            script_path = Some(a.clone());
            seen_script = true;
        } else {
            return Err(format!(
                "Unexpected argument '{a}'. Use `--` to pass args to the script."
            ));
        }
    }

    let script_path = script_path.ok_or_else(|| "Missing <script.p7>".to_string())?;
    if !script_path.ends_with(".p7") {
        return Err(format!("Script path must end with .p7: {script_path}"));
    }

    Ok((script_path, forwarded))
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().collect();
    let program_name = argv.first().map(|s| s.as_str()).unwrap_or("p7");

    let dispatch = dispatch::dispatch_from_argv(&argv);
    match dispatch.mode {
        DispatchMode::Repl { global_argv } => {
            let global_opts = match parse_global_options(&global_argv) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("{e}\nTry: {program_name} --help");
                    return ExitCode::from(1);
                }
            };

            if global_opts.help {
                print_help(program_name);
                return ExitCode::SUCCESS;
            }

            if let Err(e) = repl::run() {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }

            ExitCode::SUCCESS
        }

        DispatchMode::Script {
            global_argv,
            script_path,
            script_args,
        } => {
            let global_opts = match parse_global_options(&global_argv) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("{e}\nTry: {program_name} --help");
                    return ExitCode::from(1);
                }
            };

            if global_opts.help {
                print_help(program_name);
                return ExitCode::SUCCESS;
            }

            // Script args are parsed/forwarded per spec, but the runtime doesn't surface argv yet.
            let script_path = Path::new(&script_path);
            match run_script(script_path, &script_args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(1)
                }
            }
        }

        DispatchMode::Subcommand {
            global_argv,
            subcommand,
            subcommand_args,
        } => {
            let global_opts = match parse_global_options(&global_argv) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("{e}\nTry: {program_name} --help");
                    return ExitCode::from(1);
                }
            };

            if global_opts.help {
                print_help(program_name);
                return ExitCode::SUCCESS;
            }

            match subcommand {
                Subcommand::Help => {
                    print_help(program_name);
                    ExitCode::SUCCESS
                }
                Subcommand::Version => {
                    println!("{program_name} {}", env!("CARGO_PKG_VERSION"));
                    ExitCode::SUCCESS
                }
                Subcommand::Repl => {
                    if subcommand_args.iter().any(|a| a == "-h" || a == "--help") {
                        print_repl_help(program_name);
                        return ExitCode::SUCCESS;
                    }
                    if !subcommand_args.is_empty() {
                        eprintln!("Unexpected arguments to repl.\nTry: {program_name} repl --help");
                        return ExitCode::from(1);
                    }
                    if let Err(e) = repl::run() {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Subcommand::Test => match test_harness::run_cli("p7 test", &subcommand_args) {
                    Ok(summary) => {
                        if summary.failed > 0 {
                            ExitCode::from(1)
                        } else {
                            ExitCode::SUCCESS
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        ExitCode::from(1)
                    }
                },
                Subcommand::Run => match parse_run_args(&subcommand_args) {
                    Ok((script_path, script_args)) => {
                        let script_path = Path::new(&script_path);
                        match run_script(script_path, &script_args) {
                            Ok(()) => ExitCode::SUCCESS,
                            Err(e) => {
                                eprintln!("{e}");
                                ExitCode::from(1)
                            }
                        }
                    }
                    Err(e) if e == "__HELP__" => {
                        print_run_help(program_name);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{e}\n");
                        print_run_help(program_name);
                        ExitCode::from(1)
                    }
                },
            }
        }
    }
}
