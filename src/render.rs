use crate::{output, world::PureWorld, AppResult};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};
use typst_layout::PagedDocument;
use typst_render::RenderOptions;

pub const PIXELS_PER_PT: f64 = 3.0;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;
pub const MAX_IMAGE_DIMENSION: u32 = 32_768;

pub fn compile_document(markup: String, root: &Path) -> AppResult<PagedDocument> {
    let world = PureWorld::new(markup, root)?;
    let compiled = typst::compile::<PagedDocument>(&world);
    for warning in compiled.warnings {
        eprintln!("Typst 警告: {}", warning.message);
    }
    compiled.output.map_err(|errors| {
        let messages = errors
            .into_iter()
            .map(|error| error.message.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        format!("原生排版编译失败:\n{messages}").into()
    })
}

pub fn check_dimensions(width_pt: f64, height_pt: f64, scale: f64) -> AppResult<(u32, u32)> {
    if !width_pt.is_finite()
        || !height_pt.is_finite()
        || !scale.is_finite()
        || width_pt <= 0.0
        || height_pt <= 0.0
        || scale <= 0.0
    {
        return Err("排版产生无效的图像尺寸或缩放比例".into());
    }
    let width = (width_pt * scale).ceil();
    let height = (height_pt * scale).ceil();
    if !width.is_finite()
        || !height.is_finite()
        || width > MAX_IMAGE_DIMENSION as f64
        || height > MAX_IMAGE_DIMENSION as f64
        || width * height > MAX_IMAGE_PIXELS as f64
    {
        return Err(format!("图像过大（约 {width:.0} × {height:.0} 像素）；单边最多 {} 像素、总计最多 {} 像素，请拆分文档",
            MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS).into());
    }
    Ok((width as u32, height as u32))
}

pub fn render_to_files(markup: String, root: &Path, stem: &OsStr) -> AppResult<Vec<PathBuf>> {
    let document = compile_document(markup, root)?;
    export_document(&document, root, stem)
}

pub fn export_document(
    document: &PagedDocument,
    root: &Path,
    stem: &OsStr,
) -> AppResult<Vec<PathBuf>> {
    if document.pages().is_empty() {
        return Err("生成的物理文档没有页面".into());
    }
    // Check every page before allocating a raster or writing any output.
    for page in document.pages() {
        let size = page.frame.size();
        check_dimensions(size.x.to_pt(), size.y.to_pt(), PIXELS_PER_PT)?;
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        let result = (|| -> AppResult<PathBuf> {
            let pixmap = typst_render::render(
                page,
                &RenderOptions {
                    pixel_per_pt: PIXELS_PER_PT.into(),
                    render_bleed: false,
                },
            );
            // Encode before reserving a final filename; encoding errors create no output.
            let png = pixmap
                .encode_png()
                .map_err(|e| format!("PNG 编码失败: {e}"))?;
            let mut page_stem = OsString::from(stem);
            if index > 0 {
                page_stem.push(format!(" - page {}", index + 1));
            }
            Ok(output::save_unique_png(root, &page_stem, &png)?)
        })();
        match result {
            Ok(path) => paths.push(path),
            Err(error) => {
                let saved = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(format!(
                    "第 {} 页输出失败: {error}\n本次此前已生成的文件（不会删除）:\n{saved}",
                    index + 1
                )
                .into());
            }
        }
    }
    Ok(paths)
}
