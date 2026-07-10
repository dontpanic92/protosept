mod dispatch;
mod module_provider;
mod project;
mod repl;
mod test_harness;

use dispatch::{DispatchMode, Subcommand};
use module_provider::FileSystemModuleProvider;
use project::{PackageKind, Project};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

#[derive(Debug, Default, Clone, Copy)]
struct GlobalOptions {
    help: bool,
    verbose: bool,
}

fn print_help(program_name: &str) {
    println!(
        "Usage:\n  {program_name} [global-options] <script.p7> [script-args...]\n  {program_name} [global-options] <subcommand> [args...]\n  {program_name} [global-options]\n\nSubcommands:\n  run [project-dir|script.p7] [-- program-args...]\n  check [project-dir]\n  build [project-dir]\n  test [test-file]\n  repl\n  version\n  help\n\nGlobal options:\n  -h, --help      Show help\n  -v, --verbose   Verbose output (currently a no-op)\n\nNotes:\n  - Direct script mode forwards all tokens after the script path verbatim.\n  - `p7 foo.p7 --help` must not trigger p7 help; it is forwarded to the script.\n  - `p7 run` loads p7.toml from the current directory.\n  - `p7 run foo.p7 -- -n 1` uses `--` to forward args in subcommand mode."
    );
}

fn print_run_help(program_name: &str) {
    println!(
        "Usage:\n  {program_name} run [project-dir|script.p7] [-- program-args...]\n\nNotes:\n  - With no path, `run` loads p7.toml from the current directory.\n  - In `run`, program args MUST come after `--`.\n  - Flags like `-n` before `--` are treated as `run` options and error if unknown."
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

    let options = p7::RunOptions { script_dir };

    p7::compile_and_run_with_provider_and_options(contents, "main", Box::new(provider), options)
        .map(|_result| ())
        .map_err(|e| format!("Error: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunTarget {
    Script(PathBuf),
    Project(PathBuf),
}

fn parse_run_args(args: &[String]) -> Result<(RunTarget, Vec<String>), String> {
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

    let target = match script_path {
        Some(path) if path.ends_with(".p7") => RunTarget::Script(PathBuf::from(path)),
        Some(path) => RunTarget::Project(PathBuf::from(path)),
        None => RunTarget::Project(PathBuf::from(".")),
    };

    Ok((target, forwarded))
}

fn parse_project_path(args: &[String]) -> Result<PathBuf, String> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Err("__HELP__".to_string());
    }
    if args.len() > 1 {
        return Err("Expected at most one project directory".to_string());
    }
    if let Some(path) = args.first() {
        if path.starts_with('-') && path != "-" {
            return Err(format!("Unknown option: {path}"));
        }
        Ok(PathBuf::from(path))
    } else {
        Ok(PathBuf::from("."))
    }
}

fn load_project(path: &Path) -> Result<Project, String> {
    let project = Project::load(path)?;
    project.validate_supported_features()?;
    project.write_lockfile()?;
    Ok(project)
}

fn run_project(path: &Path, _program_args: &[String]) -> Result<(), String> {
    let project = load_project(path)?;
    let package = project.root_package();
    if package.manifest.package.kind == PackageKind::Library {
        return Err(format!(
            "Package '{}' is a library and cannot be run",
            package.manifest.package.name
        ));
    }
    let module = project
        .compile()
        .map_err(|error| format!("Error: {error}"))?;
    let mut runtime = p7::embedding::Runtime::new();
    project.load_native_extensions(&mut runtime)?;
    runtime.set_script_dir(Some(package.root.to_string_lossy().into_owned()));
    runtime.load_module(module);
    match runtime
        .call("main", Vec::new())
        .map_err(|error| format!("Error: {error}"))?
    {
        p7::embedding::CallOutcome::Returned(_) => Ok(()),
        p7::embedding::CallOutcome::Threw(value) => Err(format!("Error: script threw {value:?}")),
        p7::embedding::CallOutcome::Trapped(error) => Err(format!("Error: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn run_rejects_library_packages() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "protosept-run-library-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).expect("create fixture");
        fs::write(
            root.join("p7.toml"),
            "[package]\nname = \"library\"\nversion = \"0.1.0\"\nkind = \"library\"\n",
        )
        .expect("write manifest");
        fs::write(root.join("src/mod.p7"), "pub fn answer() -> int { 42 }").expect("write source");

        let error = run_project(&root, &[]).expect_err("library run should fail");
        assert_eq!(error, "Package 'library' is a library and cannot be run");

        fs::remove_dir_all(root).ok();
    }
}

fn check_project(path: &Path) -> Result<(), String> {
    let project = load_project(path)?;
    project
        .compile()
        .map(|_| ())
        .map_err(|error| format!("Error: {error}"))
}

fn build_project(path: &Path) -> Result<PathBuf, String> {
    let project = load_project(path)?;
    let module = project
        .compile()
        .map_err(|error| format!("Error: {error}"))?;
    let package = project.root_package();
    let target_dir = package.root.join("target");
    fs::create_dir_all(&target_dir)
        .map_err(|error| format!("Cannot create '{}': {error}", target_dir.display()))?;
    let artifact = target_dir.join(format!(
        "{}-{}.p7bc",
        package.manifest.package.name, package.manifest.package.version
    ));
    let bytes =
        bincode::serialize(&module).map_err(|error| format!("Cannot encode bytecode: {error}"))?;
    fs::write(&artifact, bytes)
        .map_err(|error| format!("Cannot write '{}': {error}", artifact.display()))?;
    Ok(artifact)
}

fn run_tests(args: &[String]) -> anyhow::Result<test_harness::TestSummary> {
    if let Some(first) = args.first() {
        let path = PathBuf::from(first);
        let manifest_candidate = if path.file_name().is_some_and(|name| name == "p7.toml") {
            path.clone()
        } else {
            path.join("p7.toml")
        };
        if manifest_candidate.is_file() {
            let project = load_project(&path).map_err(anyhow::Error::msg)?;
            return test_harness::run_project_cli("p7 test", &args[1..], &project);
        }
    } else if Path::new("p7.toml").is_file() {
        let project = load_project(Path::new(".")).map_err(anyhow::Error::msg)?;
        return test_harness::run_project_cli("p7 test", &[], &project);
    }
    test_harness::run_cli("p7 test", args)
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
                Subcommand::Test => match run_tests(&subcommand_args) {
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
                Subcommand::Check => match parse_project_path(&subcommand_args) {
                    Ok(path) => match check_project(&path) {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(error) => {
                            eprintln!("{error}");
                            ExitCode::from(1)
                        }
                    },
                    Err(error) if error == "__HELP__" => {
                        println!("Usage:\n  {program_name} check [project-dir]");
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        ExitCode::from(1)
                    }
                },
                Subcommand::Build => match parse_project_path(&subcommand_args) {
                    Ok(path) => match build_project(&path) {
                        Ok(artifact) => {
                            println!("{}", artifact.display());
                            ExitCode::SUCCESS
                        }
                        Err(error) => {
                            eprintln!("{error}");
                            ExitCode::from(1)
                        }
                    },
                    Err(error) if error == "__HELP__" => {
                        println!("Usage:\n  {program_name} build [project-dir]");
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        ExitCode::from(1)
                    }
                },
                Subcommand::Run => match parse_run_args(&subcommand_args) {
                    Ok((target, program_args)) => {
                        let result = match target {
                            RunTarget::Script(path) => run_script(&path, &program_args),
                            RunTarget::Project(path) => run_project(&path, &program_args),
                        };
                        match result {
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
