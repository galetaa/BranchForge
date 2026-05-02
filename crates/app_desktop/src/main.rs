fn main() {
    if let Err(err) = run_main() {
        eprintln!("desktop failed: {err}");
        std::process::exit(1);
    }
}

fn run_main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") => {
            println!(
                "BranchForge Desktop\n\nUsage:\n  cargo run -p app_desktop\n  cargo run -p app_desktop -- --smoke-launch\n"
            );
            Ok(())
        }
        Some("--version") | Some("-V") => {
            println!("BranchForge Desktop {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--smoke-launch") => app_desktop::smoke_launch().map(|message| {
            println!("{message}");
        }),
        Some(arg) => Err(format!("unknown argument `{arg}`")),
        None => app_desktop::run().map_err(|err| err.to_string()),
    }
}
