use std::{path::{Path, PathBuf}, sync::{Arc, LazyLock, Mutex}};

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
use pdfium_render::prelude::*;
use ratatui::layout::Rect;
use yazi_config::YAZI;
use yazi_fs::provider::{Provider, local::Local};

use crate::Image;

static PDFIUM: LazyLock<Mutex<Pdfium>> = LazyLock::new(|| Mutex::new(Pdfium::default()));

pub struct PdfiumPdf;

impl PdfiumPdf {
	pub async fn precache_page(path: PathBuf, page: u16, cache: &Path) -> Result<()> {
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

	pub async fn downscale_page(path: PathBuf, page: u16, rect: Rect) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);

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
		max_width: u16,
		max_height: u16,
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
// 	fn from_path(path: PathBuf) -> Result<Self> {
// 		let meta =
// 			std::fs::metadata(path).with_context(|| format!("metadata failed for {}",
// path.display()))?; 		let len = meta.len();
// 		let mtime = meta.modified().unwrap_or(SystemTime::now());
// 		let mtime_secs = OffsetDateTime::from(mtime).unix_timestamp();
// 		Ok(Self { len, mtime_secs })
// 	}
// }

use hayro::{self, InterpreterSettings, Page, Pdf, RenderSettings};

static HAYRO_SETTINGS: LazyLock<InterpreterSettings> = LazyLock::new(|| InterpreterSettings {
	font_resolver: Arc::new(fontique_src::fontique_font_resolver),
	warning_sink:  Arc::new(|_| {}),
});

use std::{fs::{File, TryLockError}, io::Read};

use color::Rgba8;

type PdfData = Arc<dyn AsRef<[u8]> + Send + Sync>;

mod fontique_src;
pub struct HayroPdf;

impl HayroPdf {
	pub async fn precache_page(path: PathBuf, page: u16, cache: &Path) -> Result<()> {
		let buf = tokio::task::spawn_blocking(move || {
			let img =
				Self::render_page_from_path(&path, page, YAZI.preview.max_width, YAZI.preview.max_height)
					.with_context(|| format!("failed to render page {page}"))?;

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

	pub async fn downscale_page(path: PathBuf, page: u16, rect: Rect) -> Result<DynamicImage> {
		let (w, h) = Image::max_pixel(rect);
		let path = path.to_path_buf();

		tokio::task::spawn_blocking(move || {
			Self::render_page_from_path(&path, page, w, h)
				.with_context(|| format!("failed to render page {page}"))
		})
		.await
		.context("blocking task join error")?
	}

	fn render_page_from_path(
		path: &Path,
		page: u16,
		max_width: u16,
		max_height: u16,
	) -> Result<DynamicImage> {
		let mut file =
			File::open(path).with_context(|| format!("failed to open PDF: {}", path.display()))?;
		let data = unsafe { Self::read(&mut file)? };
		let pdf =
			Pdf::new(data).map_err(|e| anyhow!("failed to parse PDF {}: {e:?}", path.display()))?;

		Self::render_page(pdf, page, max_width, max_height)
	}

	#[inline]
	fn render_page(pdf: Pdf, page: u16, max_width: u16, max_height: u16) -> Result<DynamicImage> {
		let pages = pdf.pages();
		anyhow::ensure!(pages.len() > 0, "PDF has no pages");

		let page = pages.get(page as usize % pages.len()).unwrap();
		let pixmap =
			hayro::render(page, &HAYRO_SETTINGS, &Self::render_settings(page, max_width, max_height));
		let (width, height) = (pixmap.width(), pixmap.height());
		let container = bytemuck::cast_vec::<Rgba8, u8>(pixmap.take_unpremultiplied());

		let img = ImageBuffer::from_raw(width as u32, height as u32, container)
			.ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;

		Ok(DynamicImage::ImageRgba8(img))
	}

	#[inline]
	fn render_settings(page: &Page, max_width: u16, max_height: u16) -> RenderSettings {
		// It is Ok. The max_width and max_height could not be larger then monitor
		// dimensions which much less then f32::MAX and u16::MAX
		let max_width = max_width as f32;
		let max_height = max_height as f32;
		let (width, height) = page.render_dimensions();

		// if width <= max_width && height <= max_height {
		// 	return RenderSettings {
		// 		width: Some(width.floor() as u16),
		// 		height: Some(height.floor() as u16),
		// 		..Default::default()
		// 	};
		// }
		let scale = f32::min(max_width / width, max_height / height);
		RenderSettings {
			width:   Some((width * scale).floor() as u16),
			height:  Some((height * scale).floor() as u16),
			x_scale: scale,
			y_scale: scale,
		}
	}

	#[inline]
	unsafe fn read(file: &mut File) -> Result<PdfData> {
		match file.try_lock_shared() {
			Ok(_) => unsafe { Ok(Arc::new(memmap2::MmapOptions::new().map(&*file)?)) },
			Err(err) => match err {
				TryLockError::WouldBlock => {
					let mut buffer = Vec::new();
					file.read_to_end(&mut buffer)?;
					Ok(Arc::new(buffer))
				}
				TryLockError::Error(error) => Err(error.into()),
			},
		}
	}
}
