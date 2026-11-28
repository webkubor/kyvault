//! Write-only browser handoff. Reply only after the running, unlocked app saves.
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

fn socket(root: &Path) -> PathBuf {
    root.join("flash-bridge").join("capture.sock")
}
fn timeout(stream: &UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))
}
pub fn serve(root: &Path, save: impl Fn(&str) -> bool + Send + 'static) -> io::Result<()> {
    let dir = root.join("flash-bridge");
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        r => r?,
    }
    let meta = fs::symlink_metadata(&dir)?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let path = socket(root);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.file_type().is_socket() || UnixStream::connect(&path).is_ok() {
            return Err(io::ErrorKind::AddrInUse.into());
        }
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            if timeout(&stream).is_err() {
                continue;
            }
            let mut size = [0; 4];
            if stream.read_exact(&mut size).is_err() {
                continue;
            }
            let size = u32::from_le_bytes(size) as usize;
            if !(1..=131072).contains(&size) {
                continue;
            }
            let mut bytes = vec![0; size];
            let ok = stream.read_exact(&mut bytes).is_ok()
                && std::str::from_utf8(&bytes).is_ok_and(&save);
            let _ = stream.write_all(&[u8::from(ok)]);
        }
    });
    Ok(())
}
pub fn send(root: &Path, text: &str) -> io::Result<bool> {
    let mut stream = UnixStream::connect(socket(root))?;
    timeout(&stream)?;
    stream.write_all(&(text.len() as u32).to_le_bytes())?;
    stream.write_all(text.as_bytes())?;
    let mut result = [0];
    stream.read_exact(&mut result)?;
    Ok(result == [1])
}
