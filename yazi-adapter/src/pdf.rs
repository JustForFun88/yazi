use std::{
	path::Path,
	sync::{Arc, LazyLock, Mutex, RwLock},
};

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
	len: u64,
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

use fontdb::{self, Family, Query, Stretch, Style, Weight};
use hayro::{self, Pdf, RenderSettings, render};
use hayro_interpret::{
	self, InterpreterSettings,
	font::{FallbackFontQuery, FontData, FontQuery, FontStretch as HayroStretch, StandardFont},
};

pub static GLOBAL_FONT_DB: LazyLock<RwLock<Arc<fontdb::Database>>> =
	LazyLock::new(|| RwLock::new(Arc::new(fontdb::Database::default())));

/// This is the callback function that hayro will use to find fonts.
/// It works by querying the global fontdb.
fn hayro_font_resolver(query: &FontQuery) -> Option<(FontData, u32)> {
	// 1. Get a read-lock on the global font database
	let fontdb = GLOBAL_FONT_DB.read().ok()?;

	// 2. Convert hayro's FontQuery into a fontdb::Query
	let fontdb_query = match query {
		FontQuery::Standard(standard_font) => standard_font_query(*standard_font),
		FontQuery::Fallback(fallback_query) => match &fallback_query.font_family {
			Some(family_name) => Query {
				families: &[Family::Name(family_name)],
				weight: Weight(fallback_query.font_weight as u16),
				stretch: convert_stretch(fallback_query.font_stretch),
				style: if fallback_query.is_italic { Style::Italic } else { Style::Normal },
			},
			None => standard_font_query(fallback_query.pick_standard_font()),
		},
	};

	// 3. Try to query the database
	match fontdb.query(&fontdb_query) {
		Some(face_id) => fontdb.with_face_data(face_id, |data, index| {
			// Clone the data into an Arc for hayro's FontData type
			(Arc::new(data.to_vec()) as Arc<dyn AsRef<[u8]> + Send + Sync>, index)
		}),
		None => match query {
			FontQuery::Standard(std_font) => Some(std_font.get_font_data()),
			FontQuery::Fallback(fallback_query) => {
				Some(fallback_query.pick_standard_font().get_font_data())
			}
		},
	}
}

/// Map a [`StandardFont`] to fontdb [Query].
fn standard_font_query(std_font: StandardFont) -> Query<'static> {
	use StandardFont::*;
	let (families, weight, style): (&[Family<'static>], _, _) = match std_font {
		// Times family
		TimesRoman => (
			&[Family::Name("Times New Roman"), Family::Name("Times"), Family::Name("Liberation Serif")],
			Weight::NORMAL,
			Style::Normal,
		),
		TimesItalic => (
			&[Family::Name("Times New Roman"), Family::Name("Times"), Family::Name("Liberation Serif")],
			Weight::NORMAL,
			Style::Italic,
		),
		TimesBold => (
			&[Family::Name("Times New Roman"), Family::Name("Times"), Family::Name("Liberation Serif")],
			Weight::BOLD,
			Style::Normal,
		),
		TimesBoldItalic => (
			&[Family::Name("Times New Roman"), Family::Name("Times"), Family::Name("Liberation Serif")],
			Weight::BOLD,
			Style::Italic,
		),

		// Helvetica family → allow Arial/Liberation Sans as substitutes
		Helvetica => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::NORMAL,
			Style::Normal,
		),
		HelveticaOblique => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::NORMAL,
			Style::Italic
		),
		HelveticaBold => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::BOLD,
			Style::Normal,
		),
		HelveticaBoldOblique => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::BOLD,
			Style::Italic
		),

		// Courier family → allow Courier New/Liberation Mono
		Courier => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::NORMAL,
			Style::Normal,
		),
		CourierOblique => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::NORMAL,
			Style::Italic
		),
		CourierBold => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::BOLD,
			Style::Normal,
		),
		CourierBoldOblique => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::BOLD,
			Style::Italic
		),

		// Symbol & ZapfDingbats (names differ across OSes)
		Symbol => (
			&[Family::Name("Symbol"), Family::Name("Standard Symbols PS")],
			Weight::NORMAL,
			Style::Normal,
		),
		ZapfDingBats => (
			&[Family::Name("Zapf Dingbats"), Family::Name("ITC Zapf Dingbats")],
			Weight::NORMAL,
			Style::Normal,
		),
	};

	Query { families, weight, stretch: Stretch::Normal, style }
}

/// Converts [`HayroStretch`] enum to fontdb's [`Stretch`] enum.
fn convert_stretch(stretch: HayroStretch) -> Stretch {
	match stretch {
		HayroStretch::UltraCondensed => Stretch::UltraCondensed,
		HayroStretch::ExtraCondensed => Stretch::ExtraCondensed,
		HayroStretch::Condensed => Stretch::Condensed,
		HayroStretch::SemiCondensed => Stretch::SemiCondensed,
		HayroStretch::Normal => Stretch::Normal,
		HayroStretch::SemiExpanded => Stretch::SemiExpanded,
		HayroStretch::Expanded => Stretch::Expanded,
		HayroStretch::ExtraExpanded => Stretch::ExtraExpanded,
		HayroStretch::UltraExpanded => Stretch::UltraExpanded,
	}
}