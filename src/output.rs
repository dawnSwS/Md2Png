use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

// create_new is the synchronization point. An exists() check is NOT sufficient.
// An incomplete output is removed on ordinary write/flush/sync failures.
pub fn save_unique_png(dir: &Path, stem: &OsStr, png: &[u8]) -> io::Result<PathBuf> {
    for index in 0..=100_000_u32 {
        let mut name = OsString::from(stem);
        if index > 0 {
            name.push(format!(" ({index})"));
        }
        name.push(".png");
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                let mut pending = PendingFile {
                    path: path.clone(),
                    file: Some(file),
                    committed: false,
                };
                let file = pending.file.as_mut().expect("newly opened output file");
                file.write_all(png)?;
                file.flush()?;
                file.sync_all()?;
                pending.committed = true;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "同名输出文件过多",
    ))
}

struct PendingFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        // Windows cannot remove an open file: release the handle first.
        self.file.take();
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
