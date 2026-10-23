mod cli;
fn main() {
    std::process::exit(
        cli::dispatch(std::env::args_os().skip(1).collect())
            .unwrap_or_else(|| cli::run(std::env::args_os().skip(1).collect())),
    );
}
