use crate::Image;
use anyhow::{Result, anyhow};
use cairo::{Context, Format, ImageSurface};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
use poppler::Document;
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use yazi_config::YAZI;

pub struct PdfRenderer;

impl PdfRenderer {
	/// Precaches a PDF page by rendering it to a PNG and saving it to a cache file.
	pub async fn precache(path: &Path, page: u16, cache: PathBuf) -> Result<()> {
		let path = path.to_path_buf();
		let buf = tokio::task::spawn_blocking(move || {
			let document = Document::from_file(
				path.to_str().ok_or_else(|| anyhow!("Could not parse the path as UTF-8"))?,
				None,
			)?;
			let page =
				Self::render_page(&document, page, YAZI.preview.max_width, YAZI.preview.max_height)?;
			let mut buf = Vec::new();
			let rgba = page.into_rgba8();
			let encoder = PngEncoder::new(&mut buf);
			encoder.write_image(&rgba, rgba.width(), rgba.height(), ExtendedColorType::Rgba8)?;
			Ok::<_, anyhow::Error>(buf)
		})
		.await??;
		Ok(tokio::fs::write(cache, buf).await?)
	}

	/// Downscales a PDF page to fit within a given rectangle and returns it as a DynamicImage.
	pub async fn downscale(path: &Path, page: u16, rect: Rect) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);
		let path = path.to_path_buf();
		panic!("before");
		tokio::task::spawn_blocking(move || {
			panic!("before");
			let document = Document::from_file(
				path.to_str().ok_or_else(|| anyhow!("Could not parse the path as UTF-8"))?,
				None,
			)?;
			Self::render_page(&document, page, w, h)
		})
		.await?
		.map_err(Into::into)
	}

	/// Renders a specific PDF page to a DynamicImage with dimensions constrained by max_width and max_height.
	pub fn render_page(
		document: &Document,
		page: u16,
		max_width: u32,
		max_height: u32,
	) -> Result<DynamicImage> {
		// Ensure page index is within bounds
		let num_pages = document.n_pages();
		if num_pages == 0 {
			return Err(anyhow!("PDF document has no pages"));
		}
		let page_index = (page as i32) % num_pages;
		let page = document.page(page_index as i32).ok_or_else(|| anyhow!("Page not found"))?;

		// Get page dimensions in points
		let (page_width, page_height) = page.size();
		if page_width == 0.0 || page_height == 0.0 {
			return Err(anyhow!("Invalid page dimensions"));
		}
		let target_ratio = page_width / page_height;
		let max_ratio = max_width as f64 / max_height as f64;

		// Calculate target dimensions preserving aspect ratio
		let (target_width, target_height) = if max_ratio < target_ratio {
			let scale = max_width as f64 / page_width as f64;
			(max_width, (page_height as f64 * scale).round() as u32)
		} else {
			let scale = max_height as f64 / page_height as f64;
			((page_width as f64 * scale).round() as u32, max_height)
		};

		// Ensure dimensions are valid
		if target_width == 0 || target_height == 0 {
			return Err(anyhow!("Calculated dimensions are zero"));
		}

		// Create a Cairo surface for rendering
		let mut surface =
			ImageSurface::create(Format::ARgb32, target_width as i32, target_height as i32)?;
		let context = Context::new(&surface)?;

		// Calculate scale factor and apply it
		let scale_factor = if max_ratio < target_ratio {
			target_width as f64 / page_width
		} else {
			target_height as f64 / page_height
		};
		context.scale(scale_factor, scale_factor);

		// Set white background
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint()?;

		// Render the page to the Cairo context
		page.render(&context);

		// Create image from surface data
		let width = surface.width() as u32;
		let height = surface.height() as u32;
		let data = surface.data()?;

		let mut rgba_data = Vec::with_capacity(data.len());
		for chunk in data.chunks_exact(4) {
			rgba_data.extend_from_slice(&chunk[1..]);
		}

		ImageBuffer::from_raw(width, height, rgba_data)
			.map(DynamicImage::ImageRgb8)
			.ok_or_else(|| anyhow!("Failed to create ImageBuffer from Cairo surface"))
	}
}
