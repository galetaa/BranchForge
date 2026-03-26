fn main() {
    if let Err(err) = app_host::run_console_app() {
        eprintln!("console runner failed: {err}");
        std::process::exit(1);
    }
}
