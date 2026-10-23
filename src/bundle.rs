//! Standalone executables: a runtime binary with the program appended as a
//! payload plus a trailer (`magic`, payload length). No toolchain is needed to
//! "compile" and the artifact is a single file.

use crate::ir::Program;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"XLSLANG1";

pub fn write_executable(program: &Program, runtime: &Path, out: &Path) -> Result<(), String> {
    let payload = serde_json::to_vec(program).map_err(|e| e.to_string())?;
    let mut bytes = std::fs::read(runtime).map_err(|e| format!("{}: {e}", runtime.display()))?;
    // A runtime that already carries a payload is stripped first.
    if let Some(len) = trailer_len(&bytes) {
        bytes.truncate(bytes.len() - 16 - len);
    }
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let mut f = File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn trailer_len(bytes: &[u8]) -> Option<usize> {
    let n = bytes.len();
    if n < 16 || &bytes[n - 16..n - 8] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(bytes[n - 8..].try_into().ok()?) as usize;
    (len + 16 <= n).then_some(len)
}

/// The program embedded in the running executable, if any.
pub fn read_embedded() -> Option<Program> {
    let mut f = File::open(std::env::current_exe().ok()?).ok()?;
    let size = f.seek(SeekFrom::End(0)).ok()?;
    if size < 16 {
        return None;
    }
    f.seek(SeekFrom::End(-16)).ok()?;
    let mut tail = [0u8; 16];
    f.read_exact(&mut tail).ok()?;
    if &tail[..8] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(tail[8..].try_into().ok()?);
    if len + 16 > size {
        return None;
    }
    f.seek(SeekFrom::End(-16 - len as i64)).ok()?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}
