use mlua::{Function, IntoLuaMulti, Lua, Value};
use yazi_adapter::{ADAPTOR, HayroPdf, PdfiumPdf};
use yazi_binding::{Error, UrlRef, elements::Rect};
use yazi_fs::FsUrl;
use yazi_shared::url::{AsUrl, UrlLike};

use super::Utils;

impl Utils {
	pub(super) fn pdfium_pdf_page_show(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (url, page, rect): (UrlRef, u16, Rect)| async move {
			let path = url.as_url().unified_path();
			match ADAPTOR.get().pdfium_pdf_page_show(path, page, *rect).await {
				Ok(area) => Rect::from(area).into_lua_multi(&lua),
				Err(e) => (Value::Nil, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}

	pub(super) fn pdfium_pdf_page_precache(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (src, page, dist): (UrlRef, u16, UrlRef)| async move {
			let Some(dist) = dist.as_path() else {
				return (Value::Nil, Error::custom("Destination must be a local path"))
					.into_lua_multi(&lua);
			};
			let src = src.as_url().unified_path().into_owned();
			match PdfiumPdf::precache_page(src, page, dist).await {
				Ok(()) => true.into_lua_multi(&lua),
				Err(e) => (false, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}
}

impl Utils {
	pub(super) fn hayro_pdf_page_show(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (url, page, rect): (UrlRef, u16, Rect)| async move {
			let path = url.as_url().unified_path();
			match ADAPTOR.get().hayro_pdf_page_show(path, page, *rect).await {
				Ok(area) => Rect::from(area).into_lua_multi(&lua),
				Err(e) => (Value::Nil, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}

	pub(super) fn hayro_pdf_page_precache(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (src, page, dist): (UrlRef, u16, UrlRef)| async move {
			let Some(dist) = dist.as_path() else {
				return (Value::Nil, Error::custom("Destination must be a local path"))
					.into_lua_multi(&lua);
			};
			let src = src.as_url().unified_path().into_owned();
			match HayroPdf::precache_page(src, page, dist).await {
				Ok(()) => true.into_lua_multi(&lua),
				Err(e) => (false, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}
}
