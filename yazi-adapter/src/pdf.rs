use std::{path::Path, sync::{LazyLock, Mutex}};

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
use pdfium_render::prelude::*;
use ratatui::layout::Rect;
use yazi_config::YAZI;
use yazi_fs::provider::{Provider, local::Local};

use crate::Image;

static PDFIUM: LazyLock<Mutex<Pdfium>> = LazyLock::new(|| Mutex::new(Pdfium::default()));

pub struct PdfRenderer;

impl PdfRenderer {
	pub async fn precache_page(path: &Path, page: u16, cache: &Path) -> Result<()> {
		let path = path.to_path_buf();

		let buf = tokio::task::spawn_blocking(move || {
			let img = {
				let pdfium = PDFIUM.lock().map_err(|e| anyhow!("failed to lock global Pdfium: {e}"))?;
				let document = pdfium
					.load_pdf_from_file(&path, None)
					.with_context(|| format!("failed to open PDF: {}", path.display()))?;
				Self::render_page(document, page, YAZI.preview.max_width, YAZI.preview.max_height)
					.with_context(|| format!("failed to render page {}", page))?
			};

			let mut buf = Vec::new();
			let rgba = img.into_rgba8();
			PngEncoder::new(&mut buf)
				.write_image(&rgba, rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
				.context("failed to encode PNG")?;
			Ok::<_, anyhow::Error>(buf)
		})
		.await
		.context("blocking task join error")??;

		Local
			.write(cache, buf)
			.await
			.with_context(|| format!("failed to write cache file: {}", cache.display()))?;
		Ok(())
	}

	pub async fn downscale_page(path: &Path, page: u16, rect: Rect) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);
		let path = path.to_path_buf();

		tokio::task::spawn_blocking(move || {
			let pdfium = PDFIUM.lock().map_err(|e| anyhow!("failed to lock global Pdfium: {e}"))?;
			let document = pdfium
				.load_pdf_from_file(&path, None)
				.with_context(|| format!("failed to open PDF: {}", path.display()))?;
			Self::render_page(document, page, w, h)
				.with_context(|| format!("failed to render page {}", page))
		})
		.await
		.context("blocking task join error")?
	}

	pub fn render_page<'a>(
		document: PdfDocument<'a>,
		page: u16,
		max_width: u32,
		max_height: u32,
	) -> Result<DynamicImage> {
		anyhow::ensure!(max_width > 0 && max_height > 0, "invalid max size: {max_width}x{max_height}");

		let pages = document.pages();
		let len = pages.len();
		anyhow::ensure!(len > 0, "PDF has no pages");

		let page = pages.get(page % len)?;
		let height_inches = page.height().to_inches();
		anyhow::ensure!(height_inches > 0.0, "page height is zero");

		let page_ratio = page.width().to_inches() / height_inches;
		let render_config = if page_ratio > max_width as f32 / max_height as f32 {
			PdfRenderConfig::new().set_target_width(max_width as i32)
		} else {
			PdfRenderConfig::new().set_target_height(max_height as i32)
		};

		let pdf_bitmap = page.render_with_config(&render_config).context("failed to render bitmap")?;

		let rgba_bytes = pdf_bitmap.as_rgba_bytes();
		let width = pdf_bitmap.width() as u32;
		let height = pdf_bitmap.height() as u32;

		ImageBuffer::from_raw(width, height, rgba_bytes)
			.map(DynamicImage::ImageRgba8)
			.ok_or_else(|| anyhow!("Failed to create ImageBuffer from PdfBitmap"))
	}
}

#[derive(Debug, Clone, Copy)]
struct FileSignature {
	len:        u64,
	mtime_secs: i64,
}

// impl FileSignature {
// 	fn from_path(path: &Path) -> Result<Self> {
// 		let meta =
// 			std::fs::metadata(path).with_context(|| format!("metadata failed for {}",
// path.display()))?; 		let len = meta.len();
// 		let mtime = meta.modified().unwrap_or(SystemTime::now());
// 		let mtime_secs = OffsetDateTime::from(mtime).unix_timestamp();
// 		Ok(Self { len, mtime_secs })
// 	}
// }
