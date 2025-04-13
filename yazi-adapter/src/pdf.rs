use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
use pdfium_render::prelude::*;
use ratatui::layout::Rect;
use yazi_config::YAZI;

use crate::Image;

pub struct PdfRenderer;

impl PdfRenderer {
	pub async fn precache(path: &Path, page: u16, cache: PathBuf) -> Result<()> {
		let path = path.to_path_buf();

		let buf = tokio::task::spawn_blocking(move || {
			let pdfium = Pdfium::default();
			let document = pdfium.load_pdf_from_file(&path, None)?;

			let page =
				Self::render_page(document, page, YAZI.preview.max_width, YAZI.preview.max_height)?;

			let mut buf = Vec::new();
			let rgba = page.into_rgba8();
			let encoder = PngEncoder::new(&mut buf);
			encoder.write_image(&rgba, rgba.width(), rgba.height(), ExtendedColorType::Rgba8)?;

			Ok::<_, anyhow::Error>(buf)
		})
		.await??;

		Ok(tokio::fs::write(cache, buf).await?)
	}

	pub async fn downscale(path: &Path, rect: Rect, page: u16) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);
		let path = path.to_path_buf();

		tokio::task::spawn_blocking(move || {
			let pdfium = Pdfium::default();
			let document = pdfium.load_pdf_from_file(&path, None)?;

			Self::render_page(document, page, w, h)
		})
		.await?
		.map_err(Into::into)
	}

	pub fn render_page<'a>(
		document: PdfDocument<'a>,
		page: u16,
		max_width: u32,
		max_height: u32,
	) -> Result<DynamicImage> {
		let page = document.pages().get(page % document.pages().len())?;
		let target_ratio = page.width().to_inches() / page.height().to_inches();

		let mut render_config = PdfRenderConfig::new();
		if max_width as f32 / max_height as f32 > target_ratio {
			render_config = render_config.set_target_width(max_width as i32);
		} else {
			render_config = render_config.set_target_height(max_height as i32);
		}

		let pdf_bitmap = page.render_with_config(&render_config)?;

		let rgba_bytes = pdf_bitmap.as_rgba_bytes();
		let width = pdf_bitmap.width() as u32;
		let height = pdf_bitmap.height() as u32;

		ImageBuffer::from_raw(width, height, rgba_bytes)
			.map(DynamicImage::ImageRgba8)
			.ok_or_else(|| anyhow!("Failed to create ImageBuffer from PdfBitmap"))
	}
}
