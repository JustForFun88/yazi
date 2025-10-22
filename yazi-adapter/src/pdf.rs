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
fn fontdb_font_resolver(query: &FontQuery) -> Option<(FontData, u32)> {
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
			Style::Italic,
		),
		HelveticaBold => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::BOLD,
			Style::Normal,
		),
		HelveticaBoldOblique => (
			&[Family::Name("Helvetica"), Family::Name("Arial"), Family::Name("Liberation Sans")],
			Weight::BOLD,
			Style::Italic,
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
			Style::Italic,
		),
		CourierBold => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::BOLD,
			Style::Normal,
		),
		CourierBoldOblique => (
			&[Family::Name("Courier New"), Family::Name("Courier"), Family::Name("Liberation Mono")],
			Weight::BOLD,
			Style::Italic,
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

use font_kit::{
	family_name::FamilyName,
	handle::Handle,
	properties::{Properties, Stretch as FkStretch, Style as FkStyle, Weight as FkWeight},
	source::SystemSource,
};

pub fn make_interpreter_settings() -> InterpreterSettings {
	InterpreterSettings {
		font_resolver: Arc::new(font_kit_font_resolver),
		warning_sink: Arc::new(|_| {}),
	}
}

fn font_kit_font_resolver(query: &FontQuery) -> Option<(FontData, u32)> {
	if let Some((bytes, idx)) = resolve_with_font_kit(query) {
		return Some((bytes, idx));
	}
	// Фоллбек на встроенные шрифты hayro:
	match query {
		FontQuery::Standard(s) => Some(s.get_font_data()),
		FontQuery::Fallback(fb) => Some(fb.pick_standard_font().get_font_data()),
	}
}

fn resolve_with_font_kit(query: &FontQuery) -> Option<(Arc<dyn AsRef<[u8]> + Send + Sync>, u32)> {
	// ← создаём локально, никаких статиков
	let system_fonts = SystemSource::new();

	let (families, props) = match query {
		FontQuery::Standard(std) => {
			let (fam, wt, st) = families_for_standard(*std);
			(fam, Properties { weight: wt, stretch: FkStretch::NORMAL, style: st })
		}
		FontQuery::Fallback(fb) => {
			if let Some(name) =
				fb.post_script_name.as_ref().or(fb.font_name.as_ref()).or(fb.font_family.as_ref())
			{
				let fams = vec![FamilyName::Title(name.clone())];
				let wt = weight_from_u32(fb.font_weight, fb.is_bold);
				let st = if fb.is_italic { FkStyle::Italic } else { FkStyle::Normal };
				let stretch = hayro_stretch_to_font_kit(fb.font_stretch);
				(fams, Properties { weight: wt, stretch, style: st })
			} else {
				let (fam, wt, st) = families_for_standard(fb.pick_standard_font());
				(fam, Properties { weight: wt, stretch: FkStretch::NORMAL, style: st })
			}
		}
	};

	for fam in families {
		let handle = system_fonts.select_best_match(&[fam], &props).ok()?;
		if let Some((bytes, idx)) = bytes_from_handle(&handle) {
			return Some((bytes, idx));
		}
	}
	None
}

fn bytes_from_handle(handle: &Handle) -> Option<(Arc<dyn AsRef<[u8]> + Send + Sync>, u32)> {
	match handle {
		Handle::Path { path, font_index } => {
			let data = std::fs::read(path).ok()?;
			Some((Arc::new(data), *font_index))
		}
		Handle::Memory { bytes, font_index } => Some((bytes.clone(), *font_index)),
	}
}

/// Маппинг стандартных 14 PDF-шрифтов к семействам font-kit + желаемые свойства.
fn families_for_standard(sf: StandardFont) -> (Vec<FamilyName>, FkWeight, FkStyle) {
	use StandardFont::*;
	match sf {
		// Times family
		TimesRoman => (
			vec![
				FamilyName::Title("Times New Roman".into()),
				FamilyName::Title("Times".into()),
				FamilyName::Title("Liberation Serif".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Normal,
		),
		TimesItalic => (
			vec![
				FamilyName::Title("Times New Roman".into()),
				FamilyName::Title("Times".into()),
				FamilyName::Title("Liberation Serif".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Italic,
		),
		TimesBold => (
			vec![
				FamilyName::Title("Times New Roman".into()),
				FamilyName::Title("Times".into()),
				FamilyName::Title("Liberation Serif".into()),
			],
			FkWeight::BOLD,
			FkStyle::Normal,
		),
		TimesBoldItalic => (
			vec![
				FamilyName::Title("Times New Roman".into()),
				FamilyName::Title("Times".into()),
				FamilyName::Title("Liberation Serif".into()),
			],
			FkWeight::BOLD,
			FkStyle::Italic,
		),

		// Helvetica → Arial/Liberation Sans
		Helvetica => (
			vec![
				FamilyName::Title("Helvetica".into()),
				FamilyName::Title("Arial".into()),
				FamilyName::Title("Liberation Sans".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Normal,
		),
		HelveticaOblique => (
			vec![
				FamilyName::Title("Helvetica".into()),
				FamilyName::Title("Arial".into()),
				FamilyName::Title("Liberation Sans".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Italic, // oblique ~ italic
		),
		HelveticaBold => (
			vec![
				FamilyName::Title("Helvetica".into()),
				FamilyName::Title("Arial".into()),
				FamilyName::Title("Liberation Sans".into()),
			],
			FkWeight::BOLD,
			FkStyle::Normal,
		),
		HelveticaBoldOblique => (
			vec![
				FamilyName::Title("Helvetica".into()),
				FamilyName::Title("Arial".into()),
				FamilyName::Title("Liberation Sans".into()),
			],
			FkWeight::BOLD,
			FkStyle::Italic,
		),

		// Courier → Courier New/Liberation Mono
		Courier => (
			vec![
				FamilyName::Title("Courier New".into()),
				FamilyName::Title("Courier".into()),
				FamilyName::Title("Liberation Mono".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Normal,
		),
		CourierOblique => (
			vec![
				FamilyName::Title("Courier New".into()),
				FamilyName::Title("Courier".into()),
				FamilyName::Title("Liberation Mono".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Italic,
		),
		CourierBold => (
			vec![
				FamilyName::Title("Courier New".into()),
				FamilyName::Title("Courier".into()),
				FamilyName::Title("Liberation Mono".into()),
			],
			FkWeight::BOLD,
			FkStyle::Normal,
		),
		CourierBoldOblique => (
			vec![
				FamilyName::Title("Courier New".into()),
				FamilyName::Title("Courier".into()),
				FamilyName::Title("Liberation Mono".into()),
			],
			FkWeight::BOLD,
			FkStyle::Italic,
		),

		// Symbol & ZapfDingbats (названия могут отличаться на системах)
		Symbol => (
			vec![FamilyName::Title("Symbol".into()), FamilyName::Title("Standard Symbols PS".into())],
			FkWeight::NORMAL,
			FkStyle::Normal,
		),
		ZapfDingbats => (
			vec![
				FamilyName::Title("Zapf Dingbats".into()),
				FamilyName::Title("ITC Zapf Dingbats".into()),
			],
			FkWeight::NORMAL,
			FkStyle::Normal,
		),
	}
}

/// Вес из PDF-хинтов (fallback weight/flags) → font-kit Weight
fn weight_from_u32(pdf_weight: u32, is_bold: bool) -> FkWeight {
	if is_bold {
		return FkWeight::BOLD;
	}
	// pdf_weight обычно 100..900; font-kit принимает непрерывный диапазон (0..1000).
	// Нормализуем и ограничим.
	let clamped = pdf_weight.min(1000) as f32;
	FkWeight(clamped)
}

/// Hayro stretch → font-kit Stretch
fn hayro_stretch_to_font_kit(s: HayroStretch) -> FkStretch {
	use HayroStretch::*;
	match s {
		UltraCondensed => FkStretch::ULTRA_CONDENSED,
		ExtraCondensed => FkStretch::EXTRA_CONDENSED,
		Condensed => FkStretch::CONDENSED,
		SemiCondensed => FkStretch::SEMI_CONDENSED,
		Normal => FkStretch::NORMAL,
		SemiExpanded => FkStretch::SEMI_EXPANDED,
		Expanded => FkStretch::EXPANDED,
		ExtraExpanded => FkStretch::EXTRA_EXPANDED,
		UltraExpanded => FkStretch::ULTRA_CONDENSED,
	}
}
