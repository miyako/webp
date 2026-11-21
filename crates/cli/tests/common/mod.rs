//! Shared helpers for the CLI integration suites (matrix.rs, streams.rs).
//!
//! cargo compiles `tests/common/mod.rs` once per including test binary, so
//! these items are plain functions, not a library.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::prelude::*;

/// Walk upward from the (crate-relative) test cwd to the repo root.
pub fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        assert!(dir.pop(), "repo root not found");
    }
}

/// A file under tests/fixtures/, resolved from the repo root.
pub fn fixture(rel: &str) -> PathBuf {
    repo_root().join("tests/fixtures").join(rel)
}

/// Command under test: WEBP_CONVERT_BIN (e.g. the release or Universal
/// binary) when set, otherwise the standard test-built binary.
pub fn cmd() -> StdCommand {
    match std::env::var("WEBP_CONVERT_BIN") {
        Ok(path) => StdCommand::new(resolve_bin(&path)),
        Err(_) => StdCommand::cargo_bin("webp-convert").unwrap(),
    }
}

/// A WEBP_CONVERT_BIN relative to the repo root (e.g. target/release/...)
/// still works when cargo runs the test with cwd = crates/cli.
fn resolve_bin(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_file() {
        return p;
    }
    // manifest dir is crates/cli; the workspace target dir lives two up
    let from_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(path);
    if from_root.is_file() {
        return from_root;
    }
    p
}

/// Run with optional stdin; return the raw process output. Panics on spawn
/// failure with the full command line for context.
pub fn run_with_stdin(args: &[&str], stdin: Option<&[u8]>) -> std::process::Output {
    let mut c = cmd();
    c.args(args);
    match stdin {
        Some(bytes) => {
            use std::io::Write;
            let mut child = c
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| panic!("spawn {args:?}: {e}"));
            // write stdin first; inputs fit in the pipe buffer at this size
            child
                .stdin
                .as_mut()
                .expect("stdin piped")
                .write_all(bytes)
                .unwrap_or_else(|e| panic!("write stdin: {e}"));
            drop(child.stdin.take());
            child
                .wait_with_output()
                .unwrap_or_else(|e| panic!("wait {args:?}: {e}"))
        }
        None => c.output().unwrap_or_else(|e| panic!("run {args:?}: {e}")),
    }
}

pub fn stdout_string(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr_string(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Remaining entries of a directory (names only) — used to assert there is
/// no temp-file litter after failures.
pub fn dir_entries(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}
