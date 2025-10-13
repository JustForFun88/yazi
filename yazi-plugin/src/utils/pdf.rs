use mlua::{Function, IntoLuaMulti, Lua, Value};
use yazi_adapter::{ADAPTOR, PdfRenderer};
use yazi_binding::{Error, UrlRef, elements::Rect};

use super::Utils;

impl Utils {
	pub(super) fn pdf_page_show(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (url, page, rect): (UrlRef, u16, Rect)| async move {
			let Some(path) = url.as_path() else {
				return (Value::Nil, Error::custom("Only local files are supported")).into_lua_multi(&lua);
			};
			match ADAPTOR.get().pdf_page_show(path, page, *rect).await {
				Ok(area) => Rect::from(area).into_lua_multi(&lua),
				Err(e) => (Value::Nil, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}

	pub(super) fn pdf_page_precache(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (src, page, dist): (UrlRef, u16, UrlRef)| async move {
			let Some((src, dist)) = src.as_path().zip(dist.as_path()) else {
				return (Value::Nil, Error::custom("Only local files are supported")).into_lua_multi(&lua);
			};
			match PdfRenderer::precache_page(src, page, dist).await {
				Ok(()) => true.into_lua_multi(&lua),
				Err(e) => (false, Error::custom(e.to_string())).into_lua_multi(&lua),
			}
		})
	}
}
