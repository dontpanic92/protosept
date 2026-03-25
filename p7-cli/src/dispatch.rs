#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subcommand {
    Run,
    Test,
    Repl,
    Version,
    Help,
    // Future: check/build/fmt/etc.
}

impl Subcommand {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "run" => Some(Self::Run),
            "test" => Some(Self::Test),
            "repl" => Some(Self::Repl),
            "version" => Some(Self::Version),
            "help" => Some(Self::Help),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchMode {
    Subcommand {
        global_argv: Vec<String>,
        subcommand: Subcommand,
        subcommand_args: Vec<String>,
    },
    Script {
        global_argv: Vec<String>,
        script_path: String,
        script_args: Vec<String>,
    },
    Repl {
        global_argv: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchResult {
    pub mode: DispatchMode,
}

fn is_flag_token(token: &str) -> bool {
    token.starts_with('-') && token != "-"
}

fn is_double_dash(token: &str) -> bool {
    token == "--"
}

fn is_script_path_token(token: &str) -> bool {
    token.ends_with(".p7")
}

fn first_double_dash_index(tokens: &[String]) -> Option<usize> {
    tokens.iter().position(|t| is_double_dash(t))
}

#[allow(dead_code)]
pub fn global_help_requested(global_argv: &[String]) -> bool {
    global_argv.iter().any(|a| a == "-h" || a == "--help")
}

/// Dispatches CLI mode per specs/p7-cli.md.
///
/// - argv must include argv[0] (program name).
/// - This performs no IO and does not validate file existence.
pub fn dispatch_from_argv(argv: &[String]) -> DispatchResult {
    let program_name = argv.first().cloned().unwrap_or_else(|| "p7".to_string());

    let tokens: &[String] = if argv.len() >= 2 { &argv[1..] } else { &[] };

    // 1) Subcommand detection: scan until `--`, take first non-flag token.
    let mut subcommand_pos: Option<usize> = None;
    for (i, token) in tokens.iter().enumerate() {
        if is_double_dash(token) {
            break;
        }
        if is_flag_token(token) {
            continue;
        }
        subcommand_pos = Some(i);
        break;
    }

    if let Some(i) = subcommand_pos
        && let Some(subcommand) = Subcommand::parse(tokens[i].as_str()) {
            let global_argv = std::iter::once(program_name.clone())
                .chain(tokens[..i].iter().cloned())
                .collect::<Vec<_>>();
            let subcommand_args = tokens[i + 1..].to_vec();
            return DispatchResult {
                mode: DispatchMode::Subcommand {
                    global_argv,
                    subcommand,
                    subcommand_args,
                },
            };
        }

    // 2) Direct script detection: find first *.p7 token (continue scanning past `--`).
    let mut script_pos: Option<usize> = None;
    for (i, token) in tokens.iter().enumerate() {
        if is_double_dash(token) {
            continue;
        }
        if is_script_path_token(token) {
            script_pos = Some(i);
            break;
        }
    }

    if let Some(i) = script_pos {
        let dd = first_double_dash_index(tokens);
        let global_end = dd.map_or(i, |dd_i| dd_i.min(i));

        let global_argv = std::iter::once(program_name.clone())
            .chain(tokens[..global_end].iter().cloned())
            .collect::<Vec<_>>();

        let script_path = tokens[i].clone();
        let script_args = tokens[i + 1..].to_vec();

        return DispatchResult {
            mode: DispatchMode::Script {
                global_argv,
                script_path,
                script_args,
            },
        };
    }

    // 3) REPL mode.
    let dd = first_double_dash_index(tokens);
    let global_end = dd.unwrap_or(tokens.len());
    let global_argv = std::iter::once(program_name)
        .chain(tokens[..global_end].iter().cloned())
        .collect::<Vec<_>>();

    DispatchResult {
        mode: DispatchMode::Repl { global_argv },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn p7_starts_repl() {
        let d = dispatch_from_argv(&argv(&["p7"]));
        assert!(matches!(d.mode, DispatchMode::Repl { .. }));
    }

    #[test]
    fn p7_repl_subcommand() {
        let d = dispatch_from_argv(&argv(&["p7", "repl"]));
        match d.mode {
            DispatchMode::Subcommand {
                subcommand,
                subcommand_args,
                ..
            } => {
                assert_eq!(subcommand, Subcommand::Repl);
                assert!(subcommand_args.is_empty());
            }
            _ => panic!("expected subcommand"),
        }
    }

    #[test]
    fn script_help_is_forwarded() {
        let d = dispatch_from_argv(&argv(&["p7", "foo.p7", "--help"]));
        match d.mode {
            DispatchMode::Script {
                global_argv,
                script_path,
                script_args,
            } => {
                assert_eq!(script_path, "foo.p7");
                assert!(!global_help_requested(&global_argv));
                assert_eq!(script_args, vec!["--help".to_string()]);
            }
            _ => panic!("expected script mode"),
        }
    }

    #[test]
    fn global_help_before_script_is_global() {
        let d = dispatch_from_argv(&argv(&["p7", "--help", "foo.p7"]));
        match d.mode {
            DispatchMode::Script { global_argv, .. } => {
                assert!(global_help_requested(&global_argv));
            }
            _ => panic!("expected script mode"),
        }
    }

    #[test]
    fn run_subcommand_collects_forwarded_args() {
        let d = dispatch_from_argv(&argv(&["p7", "run", "foo.p7", "--", "-n", "1"]));
        match d.mode {
            DispatchMode::Subcommand {
                subcommand,
                subcommand_args,
                ..
            } => {
                assert_eq!(subcommand, Subcommand::Run);
                assert_eq!(
                    subcommand_args,
                    vec!["foo.p7", "--", "-n", "1"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                );
            }
            _ => panic!("expected subcommand mode"),
        }
    }

    #[test]
    fn global_flags_before_subcommand_are_allowed() {
        let d = dispatch_from_argv(&argv(&["p7", "-v", "test"]));
        match d.mode {
            DispatchMode::Subcommand {
                global_argv,
                subcommand,
                ..
            } => {
                assert_eq!(subcommand, Subcommand::Test);
                assert_eq!(global_argv, vec!["p7".to_string(), "-v".to_string()]);
            }
            _ => panic!("expected subcommand mode"),
        }
    }
}
