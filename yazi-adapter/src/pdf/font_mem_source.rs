use std::{any::Any, borrow::Borrow, collections::{HashMap, hash_map::Entry}, hash::{Hash, Hasher}, sync::Arc};

use font_kit::{error::{FontLoadingError, SelectionError}, family_handle::FamilyHandle, family_name::FamilyName, font::Font, handle::Handle, properties::Properties, source::Source};

use crate::info;

// FIXME(pcwalton): These could expand to multiple fonts, and they could be
// language-specific.
#[cfg(any(target_family = "windows", target_os = "macos", target_os = "ios"))]
const DEFAULT_FONT_FAMILY_SERIF: &'static str = "Times New Roman";
#[cfg(any(target_family = "windows", target_os = "macos", target_os = "ios"))]
const DEFAULT_FONT_FAMILY_SANS_SERIF: &'static str = "Arial";

#[cfg(any(target_family = "windows", target_os = "macos", target_os = "ios"))]
const DEFAULT_FONT_FAMILY_MONOSPACE: &'static str = "Courier New";

#[cfg(any(target_family = "windows", target_os = "macos", target_os = "ios"))]
const DEFAULT_FONT_FAMILY_CURSIVE: &'static str = "Comic Sans MS";

#[cfg(target_family = "windows")]
const DEFAULT_FONT_FAMILY_FANTASY: &'static str = "Impact";

#[cfg(any(target_os = "macos", target_os = "ios"))]
const DEFAULT_FONT_FAMILY_FANTASY: &'static str = "Papyrus";

#[cfg(not(any(target_family = "windows", target_os = "macos", target_os = "ios")))]
const DEFAULT_FONT_FAMILY_SERIF: &str = "serif";

#[cfg(not(any(target_family = "windows", target_os = "macos", target_os = "ios")))]
const DEFAULT_FONT_FAMILY_SANS_SERIF: &str = "sans-serif";

#[cfg(not(any(target_family = "windows", target_os = "macos", target_os = "ios")))]
const DEFAULT_FONT_FAMILY_MONOSPACE: &str = "monospace";

#[cfg(not(any(target_family = "windows", target_os = "macos", target_os = "ios")))]
const DEFAULT_FONT_FAMILY_CURSIVE: &str = "cursive";

#[cfg(not(any(target_family = "windows", target_os = "macos", target_os = "ios")))]
const DEFAULT_FONT_FAMILY_FANTASY: &str = "fantasy";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FamilyNameStr(pub Arc<str>);

impl Borrow<str> for FamilyNameStr {
	fn borrow(&self) -> &str { &self.0 }
}

impl From<&str> for FamilyNameStr {
	fn from(s: &str) -> Self { Self(Arc::from(s)) }
}

impl From<String> for FamilyNameStr {
	fn from(s: String) -> Self { Self(Arc::from(s)) }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PostscriptNameStr(pub Arc<str>);

impl Borrow<str> for PostscriptNameStr {
	fn borrow(&self) -> &str { &self.0 }
}

impl From<&str> for PostscriptNameStr {
	fn from(s: &str) -> Self { Self(Arc::from(s)) }
}

impl From<String> for PostscriptNameStr {
	fn from(s: String) -> Self { Self(Arc::from(s)) }
}

#[derive(Clone, Debug)]
pub struct FontInfo {
	pub family_name:     FamilyNameStr,
	pub postscript_name: PostscriptNameStr,
	pub font:            Handle,
}

pub struct MemMapSource {
	families: HashMap<FamilyNameStr, FamilyHandle>,
	ps_index: HashMap<PostscriptNameStr, smallvec::SmallVec<[FontInfo; 1]>>,
}

impl MemMapSource {
	pub fn empty() -> Self { Self { families: HashMap::new(), ps_index: HashMap::new() } }

	pub fn from_fonts<I>(fonts: I) -> Result<Self, FontLoadingError>
	where
		I: Iterator<Item = Handle>,
	{
		let mut sourse = Self::empty();
		sourse.add_fonts(fonts)?;
		Ok(sourse)
	}

	pub fn add_font(&mut self, handle: Handle) -> Result<Font, FontLoadingError> {
		let font = Font::from_handle(&handle)?;
		// Mimic font-kit behavior: only index fonts that have a PostScript name.
		if let Some(postscript_name) = font.postscript_name().map(PostscriptNameStr::from) {
			let family_name = FamilyNameStr::from(font.family_name());
			let font_info = FontInfo {
				family_name:     family_name.clone(),
				postscript_name: postscript_name.clone(),
				font:            handle.clone(),
			};
			self.families.entry(family_name).or_default().push(handle.clone());
			self.ps_index.entry(postscript_name).or_default().push(font_info);
		}

		Ok(font)
	}

	pub fn add_fonts(
		&mut self,
		handles: impl Iterator<Item = Handle>,
	) -> Result<(), FontLoadingError> {
		for handle in handles {
			self.add_font(handle)?;
		}
		Ok(())
	}

	pub fn all_fonts(&self) -> Result<Vec<Handle>, SelectionError> {
		let mut out = Vec::with_capacity(self.ps_index.len());
		out.extend(
			self.ps_index.values().map(|infos| infos.iter().map(|info| info.font.clone())).flatten(),
		);
		Ok(out)
	}

	pub fn all_families(&self) -> Result<Vec<String>, SelectionError> {
		let mut families: Vec<_> =
			self.families.keys().map(|family| String::from(&*family.0)).collect();
		families.sort_unstable();
		Ok(families)
	}

	pub fn select_family_by_name(&self, family_name: &str) -> Result<FamilyHandle, SelectionError> {
		self
			.families
			.get(family_name)
			.map(|family| FamilyHandle::from_font_handles(family.fonts().iter().cloned()))
			.ok_or(SelectionError::NotFound)
	}

	pub fn select_by_postscript_name(&self, postscript_name: &str) -> Result<Handle, SelectionError> {
		self
			.ps_index
			.get(postscript_name)
			.map(|fonts| fonts.get(0).map(|font| font.font.clone()))
			.flatten()
			.ok_or(SelectionError::NotFound)
	}

	#[inline]
	pub fn select_best_match(
		&self,
		family_names: &[FamilyName],
		properties: &Properties,
	) -> Result<Handle, SelectionError> {
		<Self as Source>::select_best_match(self, family_names, properties)
		// for family_name in family_names {
		// 	if let Ok(family_handle) = self.get_family_by_generic_name(family_name)
		// { 		let candidates =
		// self.select_descriptions_in_family(&family_handle)?; 		if let Ok(index)
		// = matching::find_best_match(&candidates, properties) {
		// 			return Ok(family_handle.fonts[index].clone());
		// 		}
		// 	}
		// }
		// Err(SelectionError::NotFound)
	}

	fn get_family_by_name(&self, family_name: &str) -> Result<&FamilyHandle, SelectionError> {
		self.families.get(family_name).ok_or(SelectionError::NotFound)
	}

	fn get_family_by_generic_name(
		&self,
		family_name: &FamilyName,
	) -> Result<&FamilyHandle, SelectionError> {
		match *family_name {
			FamilyName::Title(ref title) => self.get_family_by_name(title),
			FamilyName::Serif => self.get_family_by_name(DEFAULT_FONT_FAMILY_SERIF),
			FamilyName::SansSerif => self.get_family_by_name(DEFAULT_FONT_FAMILY_SANS_SERIF),
			FamilyName::Monospace => self.get_family_by_name(DEFAULT_FONT_FAMILY_MONOSPACE),
			FamilyName::Cursive => self.get_family_by_name(DEFAULT_FONT_FAMILY_CURSIVE),
			FamilyName::Fantasy => self.get_family_by_name(DEFAULT_FONT_FAMILY_FANTASY),
		}
	}
}

impl Source for MemMapSource {
	#[inline]
	fn all_fonts(&self) -> Result<Vec<Handle>, SelectionError> { self.all_fonts() }

	#[inline]
	fn all_families(&self) -> Result<Vec<String>, SelectionError> { self.all_families() }

	fn select_family_by_name(&self, family_name: &str) -> Result<FamilyHandle, SelectionError> {
		self.select_family_by_name(family_name)
	}

	fn select_by_postscript_name(&self, postscript_name: &str) -> Result<Handle, SelectionError> {
		self.select_by_postscript_name(postscript_name)
	}

	#[inline]
	fn as_any(&self) -> &dyn Any { self }

	#[inline]
	fn as_mut_any(&mut self) -> &mut dyn Any { self }
}

// enum FamilyEntryContainer {
// 	Single(FamilyEntry),
// 	Multiple(Vec<FamilyEntry>),
// }
