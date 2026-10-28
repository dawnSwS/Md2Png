use crate::{output, world::PureWorld, AppResult};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use typst::{
    foundations::Smart,
    layout::{Abs, Frame, FrameItem, GroupItem, Point, Size},
    syntax::Span,
    visualize::Geometry,
};
use typst_layout::{Page, PagedDocument};
use typst_render::RenderOptions;

pub const PIXELS_PER_PT: f64 = 3.0;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;
pub const MAX_IMAGE_DIMENSION: u32 = 32_768;
const MAX_OUTPUT_FILES: u64 = 10_000;
// Image tiles may require several temporary sampling buffers in addition to the
// canvas. Use a smaller working budget only after a page needs splitting.
const TILE_IMAGE_PIXELS: u64 = 10_000_000;
const PIXEL_GRID: u32 = 3;

/// Compile generated Typst markup with access to resources beneath `root`.
///
/// # Errors
/// Returns font/resource initialization errors or Typst compilation diagnostics.
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

/// Check the allocation budget for a single raster image.
///
/// Long documents are supported by [`export_document`], which checks each tile.
///
/// # Errors
/// Returns an error for invalid sizes/scales or dimensions exceeding the budget.
pub fn check_dimensions(width_pt: f64, height_pt: f64, scale: f64) -> AppResult<(u32, u32)> {
    let width = pixel_extent(width_pt, scale)?;
    let height = pixel_extent(height_pt, scale)?;
    if width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(format!(
            "单张图像过大（{width} × {height} 像素）；单边最多 {MAX_IMAGE_DIMENSION} 像素、总计最多 {MAX_IMAGE_PIXELS} 像素"
        ).into());
    }
    Ok((width, height))
}

fn pixel_extent(length: f64, scale: f64) -> AppResult<u32> {
    if !length.is_finite() || !scale.is_finite() || length <= 0.0 || scale <= 0.0 {
        return Err("排版产生无效的图像尺寸或缩放比例".into());
    }
    let pixels = (length * scale).ceil();
    if !pixels.is_finite() || pixels < 1.0 || pixels > f64::from(u32::MAX) {
        return Err("图像尺寸超出可表示范围".into());
    }
    Ok(checked_integer_pixels(pixels))
}

// The only float-to-integer conversion: pixel_extent has already checked that
// this is a finite, positive, integral value no larger than u32::MAX.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "caller validates a finite integer in 1..=u32::MAX"
)]
fn checked_integer_pixels(pixels: f64) -> u32 {
    pixels as u32
}

#[derive(Debug, Clone, Copy)]
struct PagePlan {
    height: u32,
    tile_height: u32,
}

impl PagePlan {
    fn new(page: &Page) -> AppResult<Self> {
        Self::for_dimensions(page.frame.width().to_pt(), page.frame.height().to_pt())
    }

    fn for_dimensions(width_pt: f64, height_pt: f64) -> AppResult<Self> {
        let width = pixel_extent(width_pt, PIXELS_PER_PT)?;
        let height = pixel_extent(height_pt, PIXELS_PER_PT)?;
        if width > MAX_IMAGE_DIMENSION {
            return Err(format!(
                "页面宽度 {width} 像素超过上限 {MAX_IMAGE_DIMENSION}；请缩小页面宽度"
            )
            .into());
        }
        let by_area = MAX_IMAGE_PIXELS / u64::from(width);
        let limit = u32::try_from(by_area.min(u64::from(MAX_IMAGE_DIMENSION)))?;
        // Integer-point offsets are exactly representable by Typst's rasterizer
        // at the fixed 3 px/pt scale, avoiding fractional translations at seams.
        let tile_height = if height <= limit {
            height
        } else {
            let working =
                u32::try_from((TILE_IMAGE_PIXELS / u64::from(width)).min(u64::from(limit)))?;
            working - working % PIXEL_GRID
        };
        if tile_height == 0 {
            return Err("页面宽度无法容纳一个渲染分片".into());
        }
        Ok(Self {
            height,
            tile_height,
        })
    }

    fn count(self) -> u32 {
        self.height.div_ceil(self.tile_height)
    }
}

/// Compile Markdown's generated markup and write its PNG output in reading order.
///
/// # Errors
/// Returns compilation, size-validation, encoding, or file-writing errors.
pub fn render_to_files(markup: String, root: &Path, stem: &OsStr) -> AppResult<Vec<PathBuf>> {
    let document = compile_document(markup, root)?;
    export_document(&document, root, stem)
}

/// Export short pages directly and long pages as consecutive vertical PNG tiles.
///
/// A tile is rendered from the vector layout into its own bounded canvas. The
/// complete long page is never allocated as a raster. Tile boundaries use integer
/// pixels without overlaps or gaps; a boundary can pass through a table or image.
///
/// # Errors
/// Returns invalid-size, output-count, encoding, or file-writing errors. An error
/// after a successful write lists all files already produced in this invocation.
pub fn export_document(
    document: &PagedDocument,
    root: &Path,
    stem: &OsStr,
) -> AppResult<Vec<PathBuf>> {
    if document.pages().is_empty() {
        return Err("生成的物理文档没有页面".into());
    }
    // Validate all page sizes and the complete output count before creating files.
    let plans = document
        .pages()
        .iter()
        .map(PagePlan::new)
        .collect::<AppResult<Vec<_>>>()?;
    let total: u64 = plans.iter().map(|plan| u64::from(plan.count())).sum();
    if total > MAX_OUTPUT_FILES {
        return Err(format!(
            "文档需要 {total} 张图片，超过单次上限 {MAX_OUTPUT_FILES}；请拆分输入文件"
        )
        .into());
    }
    let mut batch = output::reserve_png_batch(root, stem, usize::try_from(total)?)?;
    let mut paths = Vec::new();
    for (page, plan) in document.pages().iter().zip(plans) {
        export_page(page, plan, &mut batch, &mut paths)?;
    }
    Ok(paths)
}

fn export_page(
    page: &Page,
    plan: PagePlan,
    batch: &mut output::PngBatch,
    paths: &mut Vec<PathBuf>,
) -> AppResult<()> {
    let mut offset = 0;
    while offset < plan.height {
        let height = plan.tile_height.min(plan.height - offset);
        let number = paths.len() + 1;
        let result = save_tile(page, plan.count() > 1, offset, height, batch);
        match result {
            Ok(path) => paths.push(path),
            Err(error) => return Err(partial_output_error(number, &error.to_string(), paths)),
        }
        offset += height;
    }
    Ok(())
}

fn save_tile(
    page: &Page,
    split: bool,
    offset: u32,
    height: u32,
    batch: &mut output::PngBatch,
) -> AppResult<PathBuf> {
    let _cache = RenderCacheGuard;
    let tile = if split {
        cropped_page(page, offset, height)?
    } else {
        let mut prepared = page.clone();
        prepared.frame = crate::image_tiles::prepare_frame(&page.frame, 0, height)?;
        prepared
    };
    let rendered_page = &tile;
    let size = rendered_page.frame.size();
    check_dimensions(size.x.to_pt(), size.y.to_pt(), PIXELS_PER_PT)?;
    let pixmap = typst_render::render(rendered_page, &render_options());
    let png = pixmap
        .encode_png()
        .map_err(|error| format!("PNG 编码失败: {error}"))?;
    Ok(batch.write_next(&png)?)
}

// Evict memoized image textures after success and all ordinary error paths.
// This guard is declared before the tile buffers, so it drops after them.
struct RenderCacheGuard;

impl Drop for RenderCacheGuard {
    fn drop(&mut self) {
        comemo::evict(0);
    }
}

fn render_options() -> RenderOptions {
    RenderOptions {
        pixel_per_pt: PIXELS_PER_PT.into(),
        render_bleed: false,
    }
}

fn cropped_page(page: &Page, offset: u32, height: u32) -> AppResult<Page> {
    let mut contents = crate::image_tiles::prepare_frame(&page.frame, offset, height)?;
    if let Some(fill) = page.fill_or_white() {
        // Keep gradient/pattern backgrounds in the original page coordinate space.
        let background = Geometry::Rect(contents.size()).filled(fill);
        contents.prepend(
            Point::zero(),
            FrameItem::Shape(background, Span::detached()),
        );
    }
    let size = Size::new(
        page.frame.width(),
        Abs::pt(f64::from(height) / PIXELS_PER_PT),
    );
    let mut frame = Frame::hard(size);
    let position = Point::new(Abs::zero(), Abs::pt(-f64::from(offset) / PIXELS_PER_PT));
    frame.push(position, FrameItem::Group(GroupItem::new(contents)));
    let mut tile = page.clone();
    tile.frame = frame;
    tile.fill = Smart::Custom(None);
    Ok(tile)
}

fn partial_output_error(
    number: usize,
    error: &str,
    paths: &[PathBuf],
) -> Box<dyn std::error::Error> {
    let saved = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    format!("第 {number} 张图片输出失败: {error}\n本次此前已生成的文件（不会删除）:\n{saved}")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_respects_dimensions_and_area() -> AppResult<()> {
        let plan = PagePlan::for_dimensions(800.0, 50_000.0)?;
        assert_eq!(plan.height, 150_000);
        assert_eq!(plan.tile_height % PIXEL_GRID, 0);
        assert!(plan.count() > 1);
        assert!(u64::from(plan.tile_height) * 2_400 <= MAX_IMAGE_PIXELS);
        let narrow = PagePlan::for_dimensions(2.0, 50_000.0)?;
        assert!(narrow.tile_height <= MAX_IMAGE_DIMENSION);
        Ok(())
    }

    #[test]
    fn slices_preserve_pixels_across_text_shapes_and_gradient() -> AppResult<()> {
        let source = r"
#set page(width: 100pt, height: 123pt, margin: 0pt, fill: gradient.linear(red, blue, angle: 90deg))
#set text(size: 12pt)
#place(top + left, dx: 2pt, dy: 21pt)[Crossing text]
#place(top + left, dx: 20pt, dy: 38pt, rect(width: 20pt, height: 70pt, fill: green, stroke: none))
";
        let document = compile_document(source.into(), &std::env::current_dir()?)?;
        let page = document.pages().first().ok_or("missing test page")?;
        let whole = typst_render::render(page, &render_options());
        let mut joined = Vec::new();
        let mut offset = 0;
        while offset < whole.height() {
            let height = 72.min(whole.height() - offset);
            let cropped = cropped_page(page, offset, height)?;
            let pixmap = typst_render::render(&cropped, &render_options());
            assert_eq!(pixmap.width(), whole.width());
            assert_eq!(pixmap.height(), height);
            joined.extend_from_slice(pixmap.data());
            offset += height;
        }
        assert_eq!(joined, whole.data());
        Ok(())
    }
}
