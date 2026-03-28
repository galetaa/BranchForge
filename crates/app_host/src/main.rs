use std::io::{self, Write};

fn main() {
    if let Err(err) = run_main() {
        eprintln!("console runner failed: {err}");
        std::process::exit(1);
    }
}

fn run_main() -> Result<(), String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut render_result = false;
    let mut explicit_command = None;
    let mut command_parts = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage()?;
                return Ok(());
            }
            "--render" => render_result = true,
            "--command" => {
                let command = args
                    .next()
                    .ok_or_else(|| "--command requires a console command string".to_string())?;
                explicit_command = Some(command);
                if args.peek().is_some() {
                    return Err("unexpected arguments after --command".to_string());
                }
            }
            _ => command_parts.push(arg),
        }
    }

    let command = if let Some(command) = explicit_command {
        if !command_parts.is_empty() {
            return Err("cannot mix --command with positional console command tokens".to_string());
        }
        Some(command)
    } else if command_parts.is_empty() {
        None
    } else {
        Some(command_parts.join(" "))
    };

    if let Some(command) = command {
        let config = app_host::ConsoleRunnerConfig::from_current_env()?;
        let output = app_host::run_console_command(&command, config, render_result)?;
        if !output.stdout.is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.is_empty() {
            eprint!("{}", output.stderr);
            std::process::exit(1);
        }
        io::stdout().flush().map_err(|err| err.to_string())?;
        io::stderr().flush().map_err(|err| err.to_string())?;
        Ok(())
    } else {
        app_host::run_console_app()
    }
}

fn print_usage() -> Result<(), String> {
    println!(
        "Branchforge Console Runner\n\nUsage:\n  app_host\n  app_host --command \"run ops.dev_check\"\n  app_host [--render] <console command tokens...>\n\nExamples:\n  app_host open .\n  app_host run diagnostics.repo_capabilities\n  app_host --render --command \"run release.notes target/tmp/release_notes.md stable\"\n"
    );
    io::stdout().flush().map_err(|err| err.to_string())
}
