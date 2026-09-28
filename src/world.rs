use crate::AppResult;
use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use typst::{
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, Duration},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot},
    text::{Font, FontBook},
    utils::LazyHash,
    Library, LibraryExt, World,
};

const MAX_RESOURCE_BYTES: u64 = 32 * 1024 * 1024;

pub struct PureWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
    source_bytes: Bytes,
    root: PathBuf,
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,
}

impl PureWorld {
    pub fn new(source_text: String, root: &Path) -> AppResult<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err("文档资源根路径不是目录".into());
        }
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            fonts.extend(Font::iter(Bytes::new(data.to_vec())));
        }
        #[cfg(windows)]
        {
            // Windows may be installed on a drive other than C:.
            let windows = env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
            let dir = PathBuf::from(windows).join("Fonts");
            for name in [
                "msyh.ttc",
                "msyhbd.ttc",
                "msyhl.ttc",
                "simsun.ttc",
                "simhei.ttf",
                "segoeui.ttf",
                "segoeuib.ttf",
                "segoeuii.ttf",
                "segoeuiz.ttf",
                "seguiemj.ttf",
                "seguisym.ttf",
                "consola.ttf",
                "consolab.ttf",
                "consolai.ttf",
                "consolaz.ttf",
                "cambria.ttc",
                "cambriam.ttf",
            ] {
                load_font_file(&dir.join(name), &mut fonts);
            }
            if let Some(local) = env::var_os("LOCALAPPDATA") {
                load_font_dir(
                    &PathBuf::from(local).join("Microsoft/Windows/Fonts"),
                    &mut fonts,
                );
            }
        }
        // Optional user-installed fonts, separated using the platform PATH separator.
        if let Some(dirs) = env::var_os("MD2PNG_FONT_DIR") {
            for dir in env::split_paths(&dirs) {
                load_font_dir(&dir, &mut fonts);
            }
        }
        if fonts.is_empty() {
            return Err("没有可用字体；请启用 typst-assets 的 fonts feature".into());
        }
        let mut book = FontBook::new();
        for font in &fonts {
            book.push(font.info().clone());
        }
        let id = RootedPath::new(VirtualRoot::Project, VirtualPath::new("main.typ")?).intern();
        let source_bytes = Bytes::new(source_text.as_bytes().to_vec());
        Ok(Self {
            library: LazyHash::new(Library::builder().build()),
            book: LazyHash::new(book),
            fonts,
            source: Source::new(id, source_text),
            source_bytes,
            root,
            files: Mutex::new(HashMap::new()),
        })
    }

    fn read_resource(&self, id: FileId) -> FileResult<Bytes> {
        if !matches!(id.root(), VirtualRoot::Project) {
            return Err(FileError::AccessDenied);
        }
        let path = id.vpath().realize(&self.root).map_err(FileError::Realize)?;
        let canonical = path
            .canonicalize()
            .map_err(|e| FileError::from_io(e, &path))?;
        // Reject symlinks that lead outside the document's resource directory.
        if !canonical.starts_with(&self.root) {
            return Err(FileError::AccessDenied);
        }
        let mut file = File::open(&canonical).map_err(|e| FileError::from_io(e, &path))?;
        let metadata = file.metadata().map_err(|e| FileError::from_io(e, &path))?;
        if !metadata.is_file() {
            return Err(FileError::IsDirectory);
        }
        let mut data = Vec::new();
        (&mut file)
            .take(MAX_RESOURCE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| FileError::from_io(e, &path))?;
        if data.len() as u64 > MAX_RESOURCE_BYTES {
            return Err(FileError::Other(Some("单个图片资源超过 32 MiB".into())));
        }
        Ok(Bytes::new(data))
    }
}

fn load_font_file(path: &Path, fonts: &mut Vec<Font>) {
    if let Ok(data) = fs::read(path) {
        fonts.extend(Font::iter(Bytes::new(data)));
    }
}

fn load_font_dir(dir: &Path, fonts: &mut Vec<Font>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if path.is_file() && matches!(extension.as_str(), "ttf" | "ttc" | "otf" | "otc") {
            load_font_file(&path, fonts);
        }
    }
}

impl World for PureWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(PathBuf::from(
                id.vpath().get_without_slash(),
            )))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // The World contract requires file(main) to succeed if source(main) does.
        if id == self.source.id() {
            return Ok(self.source_bytes.clone());
        }
        let mut files = self
            .files
            .lock()
            .map_err(|_| FileError::Other(Some("资源缓存锁损坏".into())))?;
        if let Some(bytes) = files.get(&id) {
            return bytes.clone();
        }
        let bytes = self.read_resource(id);
        files.insert(id, bytes.clone());
        bytes
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}
