use std::{fs::{File, OpenOptions}, io::Write, path::{Path, PathBuf}, sync::{LazyLock, Mutex}};

use anyhow::{Result, anyhow};
use cairo::{Context, Format, ImageSurface};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
use poppler::Document;
use ratatui::layout::Rect;
// use tracing::{debug, error};
use yazi_config::YAZI;

pub static ACCESS_FILE: LazyLock<Mutex<File>> = LazyLock::new(|| {
	Mutex::new(
		OpenOptions::new()
			.create(true)
			.append(true)
			.open("D:/Rust/yazi/target/debug/access.txt")
			.expect("Failed to open access file"),
	)
});

use crate::Image;

pub struct PdfRenderer;

impl PdfRenderer {
	/// Precaches a PDF page by rendering it to a PNG and saving it to a cache
	/// file.
	pub async fn precache(path: &Path, page: u16, cache: PathBuf) -> Result<()> {
		// if let Ok(mut file) = ACCESS_FILE.lock() {
		// 	let _ = write!(file, "path.to_str(): {:?}\n\n", path.to_str());
		// }
		let path = path.to_path_buf();
		let buf = tokio::task::spawn_blocking(move || {
			// if let Ok(mut file) = ACCESS_FILE.lock() {
			// 	let _ = write!(file, "path.to_str(): {:?}\n\n", path.to_str());
			// }
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

	/// Downscales a PDF page to fit within a given rectangle and returns it as a
	/// DynamicImage.
	pub async fn downscale(path: &Path, page: u16, rect: Rect) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);
		let path = path.to_path_buf();
		// if let Ok(mut file) = ACCESS_FILE.lock() {
		// 	let _ = write!(file, "path.to_str(): {:?}\n\n", path.to_str());
		// }
		tokio::task::spawn_blocking(move || {
			let path2 = path.to_str().ok_or_else(|| anyhow!("Could not parse the path as UTF-8"))?;
			let document = Document::from_file(path2, None)?;
			panic!("document: {document:?}");
			Self::render_page(&document, page, w, h)
		})
		.await?
		.map_err(Into::into)
	}

	/// Renders a specific PDF page to a DynamicImage with dimensions constrained
	/// by max_width and max_height.
	pub fn render_page(
		document: &Document,
		page: u16,
		max_width: u32,
		max_height: u32,
	) -> Result<DynamicImage> {
		// Ensure page index is within bounds
		// if let Ok(mut file) = ACCESS_FILE.lock() {
		// 	let _ = write!(file, "document.n_pages(): {:?}\n\n", document.n_pages());
		// }
		let num_pages = document.n_pages();
		let page_index = (page as i32) % num_pages;
		let page = document.page(page_index as i32).ok_or_else(|| anyhow!("Page not found"))?;

		// Get page dimensions in points
		let (page_width, page_height) = page.size();
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

// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================

// use std::path::{Path, PathBuf};

// use anyhow::{Result, anyhow};
// use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder,
// codecs::png::PngEncoder}; use pdfium_render::prelude::*;
// use ratatui::layout::Rect;
// use yazi_config::YAZI;

// use crate::Image;

// pub struct PdfRenderer;

// impl PdfRenderer {
// 	pub async fn precache(path: &Path, page: u16, cache: PathBuf) -> Result<()>
// { 		let path = path.to_path_buf();

// 		let buf = tokio::task::spawn_blocking(move || {
// 			let pdfium = Pdfium::default();
// 			let document = pdfium.load_pdf_from_file(&path, None)?;

// 			let page =
// 				Self::render_page(document, page, YAZI.preview.max_width,
// YAZI.preview.max_height)?;

// 			let mut buf = Vec::new();
// 			let rgba = page.into_rgba8();
// 			let encoder = PngEncoder::new(&mut buf);
// 			encoder.write_image(&rgba, rgba.width(), rgba.height(),
// ExtendedColorType::Rgba8)?;

// 			Ok::<_, anyhow::Error>(buf)
// 		})
// 		.await??;

// 		Ok(tokio::fs::write(cache, buf).await?)
// 	}

// 	pub async fn downscale(path: &Path, page: u16, rect: Rect) ->
// Result<DynamicImage> { 		let (w, h) = Image::max_pixel(rect);
// 		let path = path.to_path_buf();

// 		tokio::task::spawn_blocking(move || {
// 			let pdfium = Pdfium::default();
// 			let document = pdfium.load_pdf_from_file(&path, None)?;

// 			Self::render_page(document, page, w, h)
// 		})
// 		.await?
// 		.map_err(Into::into)
// 	}

// 	pub fn render_page<'a>(
// 		document: PdfDocument<'a>,
// 		page: u16,
// 		max_width: u32,
// 		max_height: u32,
// 	) -> Result<DynamicImage> {
// 		let page = document.pages().get(page % document.pages().len())?;
// 		let target_ratio = page.width().to_inches() / page.height().to_inches();

// 		let mut render_config = PdfRenderConfig::new();
// 		if (max_width as f32 / max_height as f32) < target_ratio {
// 			render_config = render_config.set_target_width(max_width as i32);
// 		} else {
// 			render_config = render_config.set_target_height(max_height as i32);
// 		}

// 		let pdf_bitmap = page.render_with_config(&render_config)?;

// 		let rgba_bytes = pdf_bitmap.as_rgba_bytes();
// 		let width = pdf_bitmap.width() as u32;
// 		let height = pdf_bitmap.height() as u32;

// 		ImageBuffer::from_raw(width, height, rgba_bytes)
// 			.map(DynamicImage::ImageRgba8)
// 			.ok_or_else(|| anyhow!("Failed to create ImageBuffer from PdfBitmap"))
// 	}
// }

// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================
// ==========================================================================================

// use std::path::{Path, PathBuf};

// use anyhow::{Error, Result, anyhow};
// use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder,
// codecs::png::PngEncoder}; use mupdf::{Colorspace, Matrix, pdf::PdfDocument};
// use ratatui::layout::Rect;
// use yazi_config::YAZI;

// use crate::Image;

// pub struct PdfRenderer;

// impl PdfRenderer {
// 	pub async fn precache(path: &Path, page: u16, cache: PathBuf) -> Result<()>
// { 		let path = path.to_path_buf();

// 		let buf = tokio::task::spawn_blocking(move || {
// 			let document =
// 				PdfDocument::open(path.to_str().ok_or_else(|| anyhow!("Failed to parse
// path"))?)?;

// 			let img =
// 				Self::render_page(&document, dbg!(page), YAZI.preview.max_width,
// YAZI.preview.max_height)?;

// 			let mut buf = Vec::new();
// 			let rgba = img.into_rgba8();
// 			let encoder = PngEncoder::new(&mut buf);
// 			encoder.write_image(&rgba, rgba.width(), rgba.height(),
// ExtendedColorType::Rgba8)?;

// 			Ok::<_, Error>(buf)
// 		})
// 		.await??;

// 		tokio::fs::write(cache, buf).await?;
// 		Ok(())
// 	}

// 	pub async fn downscale(path: &Path, page: u16, rect: Rect) ->
// Result<DynamicImage> { 		let (w, h) = Image::max_pixel(rect);
// 		let path = path.to_path_buf();

// 		tokio::task::spawn_blocking(move || {
// 			let document =
// 				PdfDocument::open(path.to_str().ok_or_else(|| anyhow!("Failed to parse
// path"))?)?;

// 			Self::render_page(&document, page, w, h)
// 		})
// 		.await?
// 	}

// 	fn render_page(
// 		document: &PdfDocument,
// 		page_number: u16,
// 		max_width: u32,
// 		max_height: u32,
// 	) -> Result<DynamicImage> {
// 		let page_number = page_number % document.page_count()? as u16;
// 		let page = document.load_page(page_number as i32)?;

// 		let bounds = page.bounds()?;
// 		let page_ratio = bounds.width() / bounds.height();

// 		let (w, h) = if (max_width as f32 / max_height as f32) < page_ratio {
// 			let scale = max_width as f32 / bounds.width();
// 			(max_width, (bounds.height() * scale) as u32)
// 		} else {
// 			let scale = max_height as f32 / bounds.height();
// 			((bounds.width() * scale) as u32, max_height)
// 		};

// 		let matrix = Matrix::new_scale(w as f32 / bounds.width(), h as f32 /
// bounds.height());

// 		let pixmap = page.to_pixmap(&matrix, &Colorspace::device_rgb(), 1.0,
// true)?;

// 		let mut container = pixmap.samples().to_vec();

// 		for rgba in container.chunks_exact_mut(4) {
// 			if rgba[3] == 0 {
// 				rgba.copy_from_slice(&[255, 255, 255, 255]);
// 			}
// 		}

// 		ImageBuffer::from_raw(pixmap.width(), pixmap.height(), container)
// 			.map(DynamicImage::ImageRgba8)
// 			.ok_or_else(|| anyhow!("Failed to create ImageBuffer from PdfBitmap"))
// 	}
// }
