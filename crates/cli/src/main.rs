//! webp-convert binary entry point.
//!
//! Exit codes (plan §7.5): 0 success, 2 invalid arguments, 3
//! corrupt/unreadable/unsupported input, 4 resource limit, 5 output error,
//! 6 internal error, 7 protocol error. Usage errors from clap also exit 2.

mod args;
mod engine;
mod io;
mod registry;

fn main() {
    // Wildcard expansion is not performed on unix; on Windows the shell
    // usually covers it. Parse errors exit inside parse_or_exit.
    let argv: Vec<String> = std::env::args().collect();
    let (cli, _command) = args::parse_or_exit(argv);
    let code = engine::run(&cli);
    std::process::exit(code);
}
