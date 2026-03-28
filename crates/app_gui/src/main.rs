fn main() {
    if let Err(err) = run_main() {
        eprintln!("gui server failed: {err}");
        std::process::exit(1);
    }
}

fn run_main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut config = app_gui::GuiServerConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                config.bind = args
                    .next()
                    .ok_or_else(|| "--bind requires host:port".to_string())?;
            }
            "--help" | "-h" => {
                println!(
                    "Branchforge GUI\n\nUsage:\n  cargo run -p app_gui\n  cargo run -p app_gui -- --bind 127.0.0.1:8787\n"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument `{arg}`")),
        }
    }

    app_gui::run_gui_server(config)
}
