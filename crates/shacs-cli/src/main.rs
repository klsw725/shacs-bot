fn main() {
    let result = shacs_cli::run_from_env();
    shacs_cli::browser::shutdown_process_browser();
    match result {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
