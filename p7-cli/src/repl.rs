use std::io::{self, Write};

pub fn run() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    println!("p7 REPL (minimal). Type :q to quit.");

    loop {
        write!(stdout, "p7> ")?;
        stdout.flush()?;

        let mut line = String::new();
        let bytes = stdin.read_line(&mut line)?;
        if bytes == 0 {
            // EOF
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        if matches!(input, ":q" | ":quit" | "exit" | "quit") {
            break;
        }

        println!("(REPL not implemented yet) you typed: {input}");
    }

    Ok(())
}
