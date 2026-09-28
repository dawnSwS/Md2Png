#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use md2png::{
    input::{self, Request},
    markdown, render, AppResult,
};
use std::{env, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Md2Png: {error}");
            #[cfg(windows)]
            if env::var_os("MD2PNG_NO_DIALOG").as_deref() != Some(std::ffi::OsStr::new("1")) {
                let _ = msgbox::create("Md2Png 错误", &error.to_string(), msgbox::IconType::Error);
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> AppResult<()> {
    let request = input::parse_args(env::args_os().skip(1))?;
    if matches!(&request, Request::Help) {
        println!(
            "Md2Png\n\
            用法: md2png [Markdown 文件]\n\
                  md2png --clipboard-dir <输出目录>\n\
                  md2png -- <文件名>\n\
            不带参数时从剪贴板读取，输出到当前工作目录。\n\
            目录右键菜单会显式传入所在目录。\n\
            文件内容必须为 UTF-8（支持 UTF-8 BOM）。"
        );
        return Ok(());
    }
    let input = input::load(request)?;
    let markup = markdown::md_to_typst(&input.content)?;
    let paths = render::render_to_files(markup, &input.root, &input.stem)?;
    for path in paths {
        println!("已生成: {}", path.display());
    }
    Ok(())
}
