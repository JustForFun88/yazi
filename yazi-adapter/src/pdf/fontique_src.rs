use std::sync::{LazyLock, Mutex};

use fontique::{Attributes, Collection, CollectionOptions, FontStyle, FontWeight, FontWidth, QueryFamily, QueryStatus, SourceCache};
use hayro_interpret::{self, font::{FontData, FontQuery, FontStretch, StandardFont}};

// ---------- Windows ----------
#[cfg(target_family = "windows")]
pub const DEFAULT_HELVETICA: &str = "Arial";
#[cfg(target_family = "windows")]
pub const DEFAULT_COURIER: &str = "Courier New";
#[cfg(target_family = "windows")]
pub const DEFAULT_TIMES: &str = "Times New Roman";
#[cfg(target_family = "windows")]
pub const DEFAULT_SYMBOL: &str = "Symbol";
#[cfg(target_family = "windows")]
pub const DEFAULT_ZAPF_DINGBATS: &str = "Zapf Dingbats";

// ---------- macOS ----------
#[cfg(any(target_os = "macos"))]
pub const DEFAULT_HELVETICA: &str = "Helvetica";
#[cfg(any(target_os = "macos"))]
pub const DEFAULT_COURIER: &str = "Courier";
#[cfg(any(target_os = "macos"))]
pub const DEFAULT_TIMES: &str = "Times";
#[cfg(any(target_os = "macos"))]
pub const DEFAULT_SYMBOL: &str = "Symbol";
#[cfg(any(target_os = "macos"))]
pub const DEFAULT_ZAPF_DINGBATS: &str = "Zapf Dingbats";

// ---------- Linux ----------
#[cfg(not(any(target_family = "windows", target_os = "macos")))]
pub const DEFAULT_HELVETICA: &str = "sans-serif";
#[cfg(not(any(target_family = "windows", target_os = "macos")))]
pub const DEFAULT_COURIER: &str = "monospace";
#[cfg(not(any(target_family = "windows", target_os = "macos")))]
pub const DEFAULT_TIMES: &str = "serif";
#[cfg(not(any(target_family = "windows", target_os = "macos")))]
pub const DEFAULT_SYMBOL: &str = "Symbol";
#[cfg(not(any(target_family = "windows", target_os = "macos")))]
pub const DEFAULT_ZAPF_DINGBATS: &str = "Zapf Dingbats";

pub static GLOBAL_FONT_COLLECTION: LazyLock<Mutex<Collection>> = LazyLock::new(|| {
	let opts = CollectionOptions { system_fonts: true, shared: true };
	Mutex::new(Collection::new(opts))
});

// Shared, thread-safe cache for font data loads (requires "std" feature).
pub static GLOBAL_SOURCE_CACHE: LazyLock<Mutex<SourceCache>> =
	LazyLock::new(|| Mutex::new(SourceCache::new_shared()));

/// fontique-based resolver for hayro
fn fontique_font_resolver(hayro_query: &FontQuery) -> Option<(FontData, u32)> {
	let mut out: Option<(FontData, u32)> = None;
	{
		let mut collection = GLOBAL_FONT_COLLECTION.lock().ok()?;
		let mut cache = GLOBAL_SOURCE_CACHE.lock().ok()?;

		let mut fontique_query = collection.query(&mut *cache);

		match hayro_query {
			FontQuery::Standard(std_font) => {
				let (families, weight, style) = standard_font_query(*std_font);
				fontique_query.set_families(families);
				fontique_query.set_attributes(Attributes::new(FontWidth::NORMAL, style, weight));
			}
			FontQuery::Fallback(fallback_query) => {
				if let Some(family) = &fallback_query.font_family {
					fontique_query.set_families([family.as_str()]);
					let weight = FontWeight::new(fallback_query.font_weight as f32);
					let style = if fallback_query.is_italic { FontStyle::Italic } else { FontStyle::Normal };
					fontique_query.set_attributes(Attributes::new(
						convert_stretch(fallback_query.font_stretch),
						style,
						weight,
					));
				} else {
					let (families, weight, style) = standard_font_query(fallback_query.pick_standard_font());
					fontique_query.set_families(families);
					fontique_query.set_attributes(Attributes::new(
						convert_stretch(fallback_query.font_stretch),
						style,
						weight,
					));
				}
			}
		}

		// Take the first acceptable match and return its bytes and TTC/collection
		// index.
		fontique_query.matches_with(|candidate| {
			// Cand.blob contains shared font data, so cloning are cheap.
			let (arc, _id) = candidate.blob.clone().into_raw_parts();
			out = Some((arc, candidate.index));
			QueryStatus::Stop
		});
	}

	out.or_else(|| match hayro_query {
		FontQuery::Standard(std_font) => Some(std_font.get_font_data()),
		FontQuery::Fallback(fallback_qry) => Some(fallback_qry.pick_standard_font().get_font_data()),
	})
}

/// Map StandardFont → (preferred families, weight, style) for matching.
fn standard_font_query(
	std_font: StandardFont,
) -> (impl Iterator<Item = QueryFamily<'static>>, FontWeight, FontStyle) {
	use StandardFont::*;
	let (names, weight, style) = match std_font {
		// Times family -> allow also Liberation Serif
		TimesRoman => (
			[QueryFamily::Named(DEFAULT_TIMES), QueryFamily::Named("Liberation Serif")],
			FontWeight::NORMAL,
			FontStyle::Normal,
		),
		TimesItalic => (
			[QueryFamily::Named(DEFAULT_TIMES), QueryFamily::Named("Liberation Serif")],
			FontWeight::NORMAL,
			FontStyle::Italic,
		),
		TimesBold => (
			[QueryFamily::Named(DEFAULT_TIMES), QueryFamily::Named("Liberation Serif")],
			FontWeight::BOLD,
			FontStyle::Normal,
		),
		TimesBoldItalic => (
			[QueryFamily::Named(DEFAULT_TIMES), QueryFamily::Named("Liberation Serif")],
			FontWeight::BOLD,
			FontStyle::Italic,
		),

		// Helvetica family → -> allow also Liberation Sans
		Helvetica => (
			[QueryFamily::Named(DEFAULT_HELVETICA), QueryFamily::Named("Liberation Sans")],
			FontWeight::NORMAL,
			FontStyle::Normal,
		),
		HelveticaOblique => (
			[QueryFamily::Named(DEFAULT_HELVETICA), QueryFamily::Named("Liberation Sans")],
			FontWeight::NORMAL,
			FontStyle::Italic,
		),
		HelveticaBold => (
			[QueryFamily::Named(DEFAULT_HELVETICA), QueryFamily::Named("Liberation Sans")],
			FontWeight::BOLD,
			FontStyle::Normal,
		),
		HelveticaBoldOblique => (
			[QueryFamily::Named(DEFAULT_HELVETICA), QueryFamily::Named("Liberation Sans")],
			FontWeight::BOLD,
			FontStyle::Italic,
		),

		// Courier family -> allow also Liberation Mono
		Courier => (
			[QueryFamily::Named(DEFAULT_COURIER), QueryFamily::Named("Liberation Mono")],
			FontWeight::NORMAL,
			FontStyle::Normal,
		),
		CourierOblique => (
			[QueryFamily::Named(DEFAULT_COURIER), QueryFamily::Named("Liberation Mono")],
			FontWeight::NORMAL,
			FontStyle::Italic,
		),
		CourierBold => (
			[QueryFamily::Named(DEFAULT_COURIER), QueryFamily::Named("Liberation Mono")],
			FontWeight::BOLD,
			FontStyle::Normal,
		),
		CourierBoldOblique => (
			[QueryFamily::Named(DEFAULT_COURIER), QueryFamily::Named("Liberation Mono")],
			FontWeight::BOLD,
			FontStyle::Italic,
		),

		// Symbol / Zapf Dingbats
		Symbol => (
			[QueryFamily::Named(DEFAULT_SYMBOL), QueryFamily::Named("Standard Symbols PS")],
			FontWeight::NORMAL,
			FontStyle::Normal,
		),
		ZapfDingBats => (
			[QueryFamily::Named(DEFAULT_ZAPF_DINGBATS), QueryFamily::Named("Wingdings")],
			FontWeight::NORMAL,
			FontStyle::Normal,
		),
	};
	(names.into_iter(), weight, style)
}

/// Convert your Hayro stretch -> fontique width
fn convert_stretch(stretch: FontStretch) -> FontWidth {
	match stretch {
		FontStretch::UltraCondensed => FontWidth::ULTRA_CONDENSED, // 0.5
		FontStretch::ExtraCondensed => FontWidth::EXTRA_CONDENSED, // 0.625
		FontStretch::Condensed => FontWidth::CONDENSED,            // 0.75
		FontStretch::SemiCondensed => FontWidth::SEMI_CONDENSED,   // 0.875
		FontStretch::Normal => FontWidth::NORMAL,                  // 1.0
		FontStretch::SemiExpanded => FontWidth::SEMI_EXPANDED,     // 1.125
		FontStretch::Expanded => FontWidth::EXPANDED,              // 1.25
		FontStretch::ExtraExpanded => FontWidth::EXTRA_EXPANDED,   // 1.5
		FontStretch::UltraExpanded => FontWidth::ULTRA_EXPANDED,   // 2.0
	}
}
