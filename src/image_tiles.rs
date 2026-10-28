//! Bound image textures as well as the output canvas when exporting a long page.
//! Typst otherwise rasterizes an entire SVG or upscaled bitmap before clipping it.

use crate::{
    render::{MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, PIXELS_PER_PT},
    AppResult,
};
use typst::{
    foundations::{Bytes, Smart},
    layout::{Abs, Frame, FrameItem, Point, Size, Transform},
    visualize::{Image, ImageKind, ImageScaling, PixelEncoding, PixelFormat, RasterImage},
};

// One output-pixel border prevents antialiasing seams at crop boundaries. This
// bounded overhead is independent of the original image or document height.
const MAX_TEXTURE_PIXELS: u64 = MAX_IMAGE_PIXELS + 1_000_000;
const MAX_TEXTURE_SIDE: u32 = MAX_IMAGE_DIMENSION + 4;

#[derive(Clone, Copy)]
struct Rect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Rect {
    fn width(self) -> f64 {
        self.right - self.left
    }
    fn height(self) -> f64 {
        self.bottom - self.top
    }

    fn corners(self) -> [Point; 4] {
        [
            point(self.left, self.top),
            point(self.right, self.top),
            point(self.left, self.bottom),
            point(self.right, self.bottom),
        ]
    }
}

pub(crate) fn prepare_frame(frame: &Frame, offset_px: u32, height_px: u32) -> AppResult<Frame> {
    let top = f64::from(offset_px) / PIXELS_PER_PT;
    let viewport = Rect {
        left: 0.0,
        top,
        right: frame.width().to_pt(),
        bottom: top + f64::from(height_px) / PIXELS_PER_PT,
    };
    visit_frame(frame, Transform::identity(), viewport, 0)
}

fn visit_frame(
    frame: &Frame,
    transform: Transform,
    viewport: Rect,
    depth: usize,
) -> AppResult<Frame> {
    if depth > 256 {
        return Err("图片布局嵌套超过 256 层".into());
    }
    let mut result = frame.clone();
    result.clear();
    for (position, item) in frame.items() {
        let positioned = transform.pre_concat(Transform::translate(position.x, position.y));
        match item {
            FrameItem::Group(group) => {
                let mut group = group.clone();
                group.frame = visit_frame(
                    &group.frame,
                    positioned.pre_concat(group.transform),
                    viewport,
                    depth + 1,
                )?;
                result.push(*position, FrameItem::Group(group));
            }
            FrameItem::Image(_, size, _) if visible_rect(*size, positioned, viewport).is_none() => {
            }
            FrameItem::Image(image, size, span) if oversized(image, *size, positioned) => {
                if let Some((local, cropped, size)) =
                    image_tile(image, *size, positioned, viewport)?
                {
                    result.push(*position + local, FrameItem::Image(cropped, size, *span));
                }
            }
            _ => result.push(*position, item.clone()),
        }
    }
    Ok(result)
}

fn raster_scale(transform: Transform) -> f64 {
    let horizontal = transform.sx.get().hypot(transform.ky.get());
    let vertical = transform.kx.get().hypot(transform.sy.get());
    horizontal.max(vertical) * PIXELS_PER_PT
}

fn oversized(image: &Image, size: Size, transform: Transform) -> bool {
    let scale = raster_scale(transform);
    let aspect = image.width() / image.height();
    let width = (scale * size.x.to_pt().max(aspect * size.y.to_pt())).ceil();
    let height = (width / aspect).ceil();
    !width.is_finite()
        || !height.is_finite()
        || width > f64::from(MAX_IMAGE_DIMENSION)
        || height > f64::from(MAX_IMAGE_DIMENSION)
        || width * height > 40_000_000.0
}

fn visible_rect(size: Size, transform: Transform, viewport: Rect) -> Option<Rect> {
    let inverse = transform.invert()?;
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for corner in viewport.corners() {
        let local = corner.transform(inverse);
        left = left.min(local.x.to_pt());
        top = top.min(local.y.to_pt());
        right = right.max(local.x.to_pt());
        bottom = bottom.max(local.y.to_pt());
    }
    let padding = 1.0 / raster_scale(transform);
    let rect = Rect {
        left: (left - padding).max(0.0),
        top: (top - padding).max(0.0),
        right: (right + padding).min(size.x.to_pt()),
        bottom: (bottom + padding).min(size.y.to_pt()),
    };
    (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
}

fn image_tile(
    image: &Image,
    size: Size,
    transform: Transform,
    viewport: Rect,
) -> AppResult<Option<(Point, Image, Size)>> {
    let Some(rect) = visible_rect(size, transform, viewport) else {
        return Ok(None);
    };
    let scale = raster_scale(transform);
    let width = pixel_count(rect.width() * scale)?;
    let height = pixel_count(rect.height() * scale)?;
    if width > MAX_TEXTURE_SIDE
        || height > MAX_TEXTURE_SIDE
        || u64::from(width) * u64::from(height) > MAX_TEXTURE_PIXELS
    {
        return Err("旋转或缩放后的图片可见区域仍超过纹理预算；请减小图片显示宽度".into());
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("无法分配图片分片纹理")?;
    match image.kind() {
        ImageKind::Svg(svg) => {
            let sx = size.x.to_pt() / svg.width() * f64::from(width) / rect.width();
            let sy = size.y.to_pt() / svg.height() * f64::from(height) / rect.height();
            let tx = -rect.left * f64::from(width) / rect.width();
            let ty = -rect.top * f64::from(height) / rect.height();
            resvg::render(
                svg.tree(),
                sk_transform(sx, sy, tx, ty)?,
                &mut pixmap.as_mut(),
            );
        }
        ImageKind::Raster(raster) => {
            paint_raster(raster, size, rect, image.scaling(), &mut pixmap)?;
        }
        ImageKind::Pdf(_) => return Err("超大 PDF 插图请先转换为 PNG 或 SVG 后导入".into()),
    }
    let tile = image_from_pixels(image, &pixmap)?;
    Ok(Some((
        point(rect.left, rect.top),
        tile,
        Size::new(Abs::pt(rect.width()), Abs::pt(rect.height())),
    )))
}

fn paint_raster(
    raster: &RasterImage,
    size: Size,
    rect: Rect,
    scaling: Smart<ImageScaling>,
    target: &mut tiny_skia::Pixmap,
) -> AppResult<()> {
    let source_x = f64::from(raster.width()) / size.x.to_pt();
    let source_y = f64::from(raster.height()) / size.y.to_pt();
    // Include neighbouring source pixels so interpolation samples the original
    // image on both sides of a tile seam, rather than clamping at a new edge.
    let x = integer((rect.left * source_x).floor().max(0.0))?.saturating_sub(2);
    let y = integer((rect.top * source_y).floor().max(0.0))?.saturating_sub(2);
    let right = integer((rect.right * source_x).ceil())?
        .saturating_add(2)
        .min(raster.width());
    let bottom = integer((rect.bottom * source_y).ceil())?
        .saturating_add(2)
        .min(raster.height());
    let width = right.checked_sub(x).ok_or("无效图片裁切宽度")?;
    let height = bottom.checked_sub(y).ok_or("无效图片裁切高度")?;
    let raw = raster
        .dynamic()
        .crop_imm(x, y, width, height)
        .to_rgba8()
        .into_raw();
    let mut source = tiny_skia::Pixmap::new(width, height).ok_or("无法分配源图片裁切缓冲")?;
    for (pixel, &[red, green, blue, alpha]) in
        source.pixels_mut().iter_mut().zip(raw.as_chunks::<4>().0)
    {
        *pixel = tiny_skia::ColorU8::from_rgba(red, green, blue, alpha).premultiply();
    }
    let target_x = f64::from(target.width()) / rect.width();
    let target_y = f64::from(target.height()) / rect.height();
    let transform = sk_transform(
        target_x / source_x,
        target_y / source_y,
        (f64::from(x) / source_x - rect.left) * target_x,
        (f64::from(y) / source_y - rect.top) * target_y,
    )?;
    let paint = tiny_skia::PixmapPaint {
        quality: if scaling == Smart::Custom(ImageScaling::Pixelated) {
            tiny_skia::FilterQuality::Nearest
        } else {
            tiny_skia::FilterQuality::Bilinear
        },
        ..Default::default()
    };
    target.draw_pixmap(0, 0, source.as_ref(), &paint, transform, None);
    Ok(())
}

fn image_from_pixels(original: &Image, pixmap: &tiny_skia::Pixmap) -> AppResult<Image> {
    let mut bytes = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        bytes.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    let icc = match original.kind() {
        ImageKind::Raster(raster) => raster
            .icc()
            .map_or(Smart::Auto, |profile| Smart::Custom(profile.clone())),
        _ => Smart::Auto,
    };
    let format = PixelFormat {
        encoding: PixelEncoding::Rgba8,
        width: pixmap.width(),
        height: pixmap.height(),
    };
    let raster = RasterImage::new(Bytes::new(bytes), format, icc)
        .map_err(|error| format!("图片分片转换失败: {error}"))?;
    Ok(Image::new(
        raster,
        original.alt().map(Into::into),
        original.scaling(),
    ))
}

fn point(x: f64, y: f64) -> Point {
    Point::new(Abs::pt(x), Abs::pt(y))
}

fn pixel_count(value: f64) -> AppResult<u32> {
    // Abs arithmetic can produce 62.000000000007 for an exact 62-pixel crop.
    // Do not introduce a resampling shift from that floating-point roundoff.
    let nearest = value.round();
    positive_integer(if (value - nearest).abs() < 1e-7 {
        nearest
    } else {
        value.ceil()
    })
}

fn positive_integer(value: f64) -> AppResult<u32> {
    if value < 1.0 {
        return Err("无效的图片纹理尺寸".into());
    }
    integer(value)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "finite nonnegative integer is checked against u32::MAX"
)]
fn integer(value: f64) -> AppResult<u32> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err("图片坐标超出范围".into());
    }
    Ok(value as u32)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "finite f64 is checked against the f32 range required by tiny-skia"
)]
fn single(value: f64) -> AppResult<f32> {
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return Err("图片变换超出范围".into());
    }
    Ok(value as f32)
}

fn sk_transform(sx: f64, sy: f64, tx: f64, ty: f64) -> AppResult<tiny_skia::Transform> {
    Ok(tiny_skia::Transform::from_row(
        single(sx)?,
        0.0,
        0.0,
        single(sy)?,
        single(tx)?,
        single(ty)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::{syntax::Span, visualize::SvgImage};

    fn long_frame(image: Image) -> Frame {
        let size = Size::new(Abs::pt(2.0), Abs::pt(40_000.0));
        let mut frame = Frame::hard(size);
        frame.push(
            Point::zero(),
            FrameItem::Image(image, size, Span::detached()),
        );
        frame
    }

    fn assert_bounded(frame: &Frame) {
        for (_, item) in frame.items() {
            if let FrameItem::Image(image, size, _) = item {
                assert!(image.width() < 100.0);
                assert!(image.height() < 40.0);
                assert!(size.y.to_pt() < 12.0);
            }
        }
    }

    #[test]
    fn oversized_svg_uses_only_visible_texture() -> AppResult<()> {
        let data = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="40000"><rect width="2" height="40000" fill="red"/></svg>"#;
        let svg = SvgImage::new(Bytes::new(data.to_vec()))
            .map_err(|error| format!("test SVG: {error:?}"))?;
        let frame = long_frame(Image::plain(svg));
        let cropped = prepare_frame(&frame, 60_000, 30)?;
        assert_eq!(cropped.items().len(), 1);
        assert_bounded(&cropped);
        let (_, item) = cropped.items().next().ok_or("missing image")?;
        if let FrameItem::Image(image, _, _) = item {
            if let ImageKind::Raster(raster) = image.kind() {
                let pixel = raster.dynamic().to_rgba8().get_pixel(2, 15).0;
                assert_eq!(pixel, [255, 0, 0, 255]);
            }
        }
        Ok(())
    }

    #[test]
    fn oversized_bitmap_uses_only_visible_source_region() -> AppResult<()> {
        let data = [0_u8, 0, 255, 255].repeat(80_000);
        let format = PixelFormat {
            encoding: PixelEncoding::Rgba8,
            width: 2,
            height: 40_000,
        };
        let raster =
            RasterImage::plain(Bytes::new(data), format).map_err(|error| error.to_string())?;
        let frame = long_frame(Image::plain(raster));
        let cropped = prepare_frame(&frame, 60_000, 30)?;
        assert_eq!(cropped.items().len(), 1);
        assert_bounded(&cropped);
        Ok(())
    }

    #[test]
    fn invisible_oversized_image_does_not_allocate_texture() -> AppResult<()> {
        let data = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="40000"><rect width="2" height="40000" fill="red"/></svg>"#;
        let svg = SvgImage::new(Bytes::new(data.to_vec()))
            .map_err(|error| format!("test SVG: {error:?}"))?;
        let frame = long_frame(Image::plain(svg));
        let cropped = prepare_frame(&frame, 150_000, 30)?;
        assert!(cropped.is_empty());
        Ok(())
    }

    fn test_page(frame: Frame) -> typst_layout::Page {
        typst_layout::Page {
            frame,
            bleed: typst::layout::Sides::default(),
            fill: Smart::Custom(None),
            numbering: None,
            supplement: typst::foundations::Content::empty(),
            number: 1,
        }
    }

    fn assert_region_matches(frame: &Frame, offset: u32, height: u32) -> AppResult<()> {
        let options = typst_render::RenderOptions {
            pixel_per_pt: PIXELS_PER_PT.into(),
            render_bleed: false,
        };
        // Only 6 pixels wide: the full reference is a few MiB despite its height.
        let whole = typst_render::render(&test_page(frame.clone()), &options);
        let prepared = prepare_frame(frame, offset, height)?;
        let mut cropped = Frame::hard(Size::new(
            frame.width(),
            Abs::pt(f64::from(height) / PIXELS_PER_PT),
        ));
        cropped.push_frame(point(0.0, -f64::from(offset) / PIXELS_PER_PT), prepared);
        let actual = typst_render::render(&test_page(cropped), &options);
        let start = usize::try_from(offset)? * usize::try_from(whole.width())? * 4;
        let expected = whole
            .data()
            .get(start..start + actual.data().len())
            .ok_or("reference crop out of range")?;
        assert_eq!(actual.data(), expected);
        Ok(())
    }

    #[test]
    fn striped_bitmap_crop_keeps_content_positions() -> AppResult<()> {
        let mut data = Vec::new();
        for row in 0..40_000 {
            let color = match (row / 10) % 3 {
                0 => [255_u8, 0, 0, 255],
                1 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            };
            data.extend_from_slice(&color);
            data.extend_from_slice(&color);
        }
        let format = PixelFormat {
            encoding: PixelEncoding::Rgba8,
            width: 2,
            height: 40_000,
        };
        let raster =
            RasterImage::plain(Bytes::new(data), format).map_err(|error| error.to_string())?;
        let image = Image::new(raster, None, Smart::Custom(ImageScaling::Pixelated));
        let frame = long_frame(image);
        assert_region_matches(&frame, 59_970, 180)
    }

    #[test]
    fn nested_scaled_svg_crop_keeps_content_positions() -> AppResult<()> {
        let data = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="40000"><rect width="2" height="40000" fill="red"/><rect y="20000" width="2" height="10" fill="lime"/><rect y="20010" width="2" height="10" fill="blue"/></svg>"#;
        let svg = SvgImage::new(Bytes::new(data.to_vec()))
            .map_err(|error| format!("test SVG: {error:?}"))?;
        let mut nested = typst::layout::GroupItem::new(long_frame(Image::plain(svg)));
        nested.transform = Transform::scale(
            typst::layout::Ratio::new(0.5),
            typst::layout::Ratio::new(0.5),
        );
        let mut frame = Frame::hard(Size::new(Abs::pt(2.0), Abs::pt(21_000.0)));
        frame.push(point(0.5, 100.0), FrameItem::Group(nested));
        assert_region_matches(&frame, 30_285, 120)
    }

    #[test]
    fn offscreen_small_images_are_culled_without_changing_original() -> AppResult<()> {
        let data = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#;
        let svg = SvgImage::new(Bytes::new(data.to_vec()))
            .map_err(|error| format!("test SVG: {error:?}"))?;
        let mut frame = Frame::hard(Size::new(Abs::pt(2.0), Abs::pt(40_000.0)));
        frame.push(
            point(0.0, 1_000.0),
            FrameItem::Image(
                Image::plain(svg),
                Size::splat(Abs::pt(2.0)),
                Span::detached(),
            ),
        );
        assert_eq!(prepare_frame(&frame, 0, 30)?.items().len(), 0);
        assert_eq!(prepare_frame(&frame, 3_000, 30)?.items().len(), 1);
        assert_eq!(frame.items().len(), 1);
        Ok(())
    }
}
