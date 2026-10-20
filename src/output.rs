use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Save a PNG with a collision-free name, without overwriting existing files.
///
/// # Errors
/// Returns an error for invalid filenames, I/O failures, or exhausted names.
pub fn save_unique_png(dir: &Path, stem: &OsStr, png: &[u8]) -> io::Result<PathBuf> {
    reserve_png_batch(dir, stem, 1)?.write_next(png)
}

/// A sequence of PNG filenames sharing a single collision suffix.
///
/// An atomic lock-directory reservation keeps simultaneous exporters in
/// separate batches without holding one open handle per page. Final PNG files
/// are created only when written, always with `create_new` to protect files
/// that another program creates after the reservation.
pub struct PngBatch {
    paths: Vec<PathBuf>,
    next: usize,
    _reservation: BatchReservation,
}

impl PngBatch {
    /// Write the next PNG, preserving earlier outputs and externally created files.
    ///
    /// # Errors
    /// Returns an error for I/O failures, a filename taken since reservation,
    /// or writing more images than reserved.
    pub fn write_next(&mut self, png: &[u8]) -> io::Result<PathBuf> {
        let path = self.paths.get(self.next).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "输出图片超过本批次预留数量")
        })?;
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut pending = PendingFile {
            path: path.clone(),
            file: Some(file),
            committed: false,
        };
        let file = pending
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("输出文件句柄不可用"))?;
        file.write_all(png)?;
        file.flush()?;
        file.sync_all()?;
        pending.committed = true;
        self.next += 1;
        Ok(path.clone())
    }
}

struct BatchReservation(PathBuf);

impl Drop for BatchReservation {
    fn drop(&mut self) {
        // Never remove final paths here: another process may have created them.
        let _ = fs::remove_dir(&self.0);
    }
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

/// Reserve a batch name before rendering the first image.
///
/// Single images retain `name.png`; multiple images use `name - page 0001.png`.
/// If any target exists, the whole batch retries as `name (1) - page 0001.png`.
///
/// # Errors
/// Returns an error for invalid names, zero images, I/O failures, or exhausted
/// collision suffixes. Failed reservations never delete existing user files.
pub fn reserve_png_batch(dir: &Path, stem: &OsStr, count: usize) -> io::Result<PngBatch> {
    validate_stem(stem)?;
    if count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "输出批次至少需要一张图片",
        ));
    }
    for collision in 0..=100_000_u32 {
        match reserve_candidate(dir, stem, count, collision) {
            Ok(batch) => return Ok(batch),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "同名输出文件过多",
    ))
}

fn reserve_candidate(
    dir: &Path,
    stem: &OsStr,
    count: usize,
    collision: u32,
) -> io::Result<PngBatch> {
    let mut base = OsString::from(stem);
    if collision > 0 {
        base.push(format!(" ({collision})"));
    }
    let mut lock_name = OsString::from(".");
    lock_name.push(&base);
    lock_name.push(".md2png-lock");
    let lock_path = dir.join(lock_name);
    fs::create_dir(&lock_path)?;
    let reservation = BatchReservation(lock_path);
    let digits = count.to_string().len().max(4);
    let mut paths = Vec::new();
    for page in 1..=count {
        let mut name = base.clone();
        if count > 1 {
            name.push(format!(" - page {page:0digits$}"));
        }
        name.push(".png");
        let path = dir.join(name);
        check_available(&path)?;
        paths.push(path);
    }
    Ok(PngBatch {
        paths,
        next: 0,
        _reservation: reservation,
    })
}

fn check_available(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "输出名称已被使用",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate_stem(stem: &OsStr) -> io::Result<()> {
    let path_stem = Path::new(stem);
    if stem.is_empty()
        || path_stem.file_name() != Some(stem)
        || stem.to_string_lossy().contains(['/', '\\'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "输出名称必须是不含目录的非空文件名",
        ));
    }
    Ok(())
}
