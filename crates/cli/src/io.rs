//! Input/output resolution, output safety (plan §6.6), and stream plumbing.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use webp_convert_core::error::{Result, WcError};

/// Binary input source: an owned byte buffer plus a displayable origin.
pub struct Input {
    pub bytes: Vec<u8>,
    pub origin: String,
    /// filesystem path when not stdin (for alias detection)
    pub path: Option<PathBuf>,
}

pub fn read_input(spec: &str, max_bytes: u64) -> Result<Input> {
    if spec == "-" {
        // read one byte beyond the limit: take(max) would silently truncate
        // AT the limit and never trigger the check below
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| WcError::UnsupportedInput(format!("failed to read stdin: {e}")))?;
        if bytes.len() as u64 > max_bytes {
            return Err(WcError::ResourceLimit(format!(
                "input exceeds --max-input-bytes ({max_bytes})"
            )));
        }
        if bytes.is_empty() {
            return Err(WcError::UnsupportedInput("stdin was empty".into()));
        }
        return Ok(Input {
            bytes,
            origin: "<stdin>".into(),
            path: None,
        });
    }
    let path = Path::new(spec);
    let meta = fs::metadata(path)
        .map_err(|e| WcError::UnsupportedInput(format!("cannot open input {spec:?}: {e}")))?;
    if !meta.is_file() {
        return Err(WcError::UnsupportedInput(format!(
            "input {spec:?} is not a regular file"
        )));
    }
    if meta.len() > max_bytes {
        return Err(WcError::ResourceLimit(format!(
            "input is {} bytes; exceeds --max-input-bytes ({max_bytes})",
            meta.len()
        )));
    }
    let bytes = fs::read(path)
        .map_err(|e| WcError::UnsupportedInput(format!("failed to read {spec:?}: {e}")))?;
    Ok(Input {
        bytes,
        origin: spec.to_string(),
        path: Some(path.to_path_buf()),
    })
}

/// Where encoded bytes are written; stdout is explicitly binary.
pub enum Output {
    File(PathBuf),
    Stdout,
}

pub fn resolve_output(spec: &str) -> Result<Output> {
    if spec == "-" {
        return Ok(Output::Stdout);
    }
    Ok(Output::File(PathBuf::from(spec)))
}

/// Refuse to replace an existing file unless `overwrite` was given.
pub fn ensure_writable(target: &Path, overwrite: bool, input_path: Option<&Path>) -> Result<()> {
    if !overwrite && target.exists() {
        return Err(WcError::Output(format!(
            "output file {} exists; pass --overwrite to replace it",
            target.display()
        )));
    }
    // aliasing: same canonical file as the input (also catches symlink tricks)
    if let Some(in_path) = input_path {
        let out_canon = target.canonicalize().unwrap_or_default();
        let in_canon = in_path.canonicalize().unwrap_or_default();
        if !out_canon.as_os_str().is_empty() && out_canon == in_canon {
            return Err(WcError::Output(
                "output path refers to the input file; refusing to replace input".into(),
            ));
        }
        // same path lexically (covers nonexistent targets pointing at the
        // input's directory entry under a lock-free race)
        if target == in_path {
            return Err(WcError::Output(
                "output path is the input path; refusing to replace input".into(),
            ));
        }
    }
    if let Some(dir) = target.parent() {
        if !dir.as_os_str().is_empty() && !dir.is_dir() {
            return Err(WcError::Output(format!(
                "output directory {} does not exist",
                dir.display()
            )));
        }
    }
    Ok(())
}

/// Atomic write: encode to a uniquely named temporary file in the target's
/// directory, then rename over the destination. Temp file removed on error.
pub fn write_output_file(target: &Path, bytes: &[u8]) -> Result<()> {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .ok_or_else(|| WcError::Output("output path has no file name".into()))?
        .to_string_lossy();
    let tmp = dir.join(format!(".webp-convert-tmp-{}-{}", std::process::id(), name));
    let write = || -> Result<()> {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| WcError::Output(format!("cannot create temp file: {e}")))?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| WcError::Output(format!("write failed: {e}")))?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        WcError::Output(format!("atomic rename failed: {e}"))
    })
}

pub fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)
        .and_then(|_| out.flush())
        .map_err(|e| WcError::Output(format!("stdout write failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_overwrite_without_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("out.bin");
        std::fs::write(&p, b"old").unwrap();
        let err = ensure_writable(&p, false, None).unwrap_err();
        assert_eq!(err.exit_code(), 5);
        ensure_writable(&p, true, None).unwrap(); // overwrite permitted
    }

    #[test]
    fn refuses_aliasing_input() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("in.png");
        std::fs::write(&p, b"x").unwrap();
        let err = ensure_writable(&p, true, Some(&p)).unwrap_err();
        assert!(err.to_string().contains("refusing to replace input"));
    }

    #[test]
    fn atomic_write_cleans_up_temp_on_readonly_target_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // target in a rewritten-always-fails place: wrong parent type
        let target = tmp.path().join("no-such-subdir/out.bin");
        let err = write_output_file(&target, b"x").unwrap_err();
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn atomic_write_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out.bin");
        write_output_file(&target, b"data").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"data");
        // no temp litter
        assert!(std::fs::read_dir(tmp.path()).unwrap().count() == 1);
    }
}
