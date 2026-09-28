use crate::AppResult;
use std::{
    env,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    path::PathBuf,
};

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Help,
    File(PathBuf),
    Clipboard(Option<PathBuf>),
}

pub struct Input {
    pub content: String,
    pub root: PathBuf,
    pub stem: OsString,
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> AppResult<Request> {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(Request::Clipboard(None)),
        [arg] if arg == OsStr::new("--help") || arg == OsStr::new("-h") => Ok(Request::Help),
        [flag, dir] if flag == OsStr::new("--clipboard-dir") => {
            if dir.is_empty() {
                return Err("输出目录不能为空".into());
            }
            Ok(Request::Clipboard(Some(PathBuf::from(dir))))
        }
        [flag, path] if flag == OsStr::new("--") => Ok(Request::File(PathBuf::from(path))),
        [path] if !path.to_string_lossy().starts_with('-') => {
            Ok(Request::File(PathBuf::from(path)))
        }
        _ => Err("参数错误。使用 md2png --help 查看用法；以 - 开头的文件名请放在 -- 后。".into()),
    }
}

pub fn load(request: Request) -> AppResult<Input> {
    match request {
        Request::File(path) => {
            // A bad explicit filename is an error, never a clipboard request.
            let mut file =
                File::open(&path).map_err(|e| format!("无法打开文件 {}: {e}", path.display()))?;
            if !file.metadata()?.is_file() {
                return Err(format!("不是普通文件: {}", path.display()).into());
            }
            let mut data = Vec::new();
            (&mut file)
                .take((MAX_INPUT_BYTES + 1) as u64)
                .read_to_end(&mut data)?;
            if data.len() > MAX_INPUT_BYTES {
                return Err("Markdown 文件超过 8 MiB，请先拆分文档".into());
            }
            let content =
                String::from_utf8(data).map_err(|_| "文件不是有效 UTF-8，请另存为 UTF-8 后重试")?;
            let full_path = std::path::absolute(&path)?;
            let root = full_path
                .parent()
                .ok_or("无法确定文件所在目录")?
                .canonicalize()?;
            let stem = path
                .file_stem()
                .unwrap_or(OsStr::new("output"))
                .to_os_string();
            Ok(Input {
                content: validate_content(content)?,
                root,
                stem,
            })
        }
        Request::Clipboard(dir) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            }
            .canonicalize()?;
            if !root.is_dir() {
                return Err(format!("输出路径不是目录: {}", root.display()).into());
            }
            let content = validate_content(read_clipboard()?)?;
            Ok(Input {
                content,
                root,
                stem: OsString::from("Markdown_NativeRender"),
            })
        }
        Request::Help => Err("帮助请求不应读取输入".into()),
    }
}

pub fn validate_content(content: String) -> AppResult<String> {
    if content.len() > MAX_INPUT_BYTES {
        return Err("Markdown 内容超过 8 MiB，请先拆分文档".into());
    }
    let content = content
        .strip_prefix('\u{feff}')
        .unwrap_or(&content)
        .to_owned();
    if content.trim().is_empty() {
        return Err("Markdown 内容为空".into());
    }
    Ok(content)
}

#[cfg(windows)]
fn read_clipboard() -> AppResult<String> {
    use arboard::Clipboard;
    use std::{thread, time::Duration};
    let mut last_error = String::new();
    for attempt in 0..3 {
        match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => return Ok(text),
            Err(error) => last_error = error.to_string(),
        }
        if attempt < 2 {
            thread::sleep(Duration::from_millis(100));
        }
    }
    Err(format!("无法读取剪贴板文本: {last_error}").into())
}

#[cfg(not(windows))]
fn read_clipboard() -> AppResult<String> {
    Err("此构建仅在 Windows 上支持剪贴板；其他平台请传入 Markdown 文件路径".into())
}
