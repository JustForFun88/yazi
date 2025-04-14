use mlua::{Function, IntoLua, Lua, Value};
use yazi_adapter::{ADAPTOR, PdfRenderer};
use yazi_binding::UrlRef;
use std::ops::{Deref, DerefMut};
use super::Utils;
use crate::elements::Rect;

impl Utils {
	// pub(super) fn image_info(lua: &Lua) -> mlua::Result<Function> {
	// 	lua.create_async_function(|lua, url: UrlRef| async move {
	// 		if let Ok(info) = yazi_adapter::ImageInfo::new(&url).await {
	// 			ImageInfo::from(info).into_lua(&lua)
	// 		} else {
	// 			Value::Nil.into_lua(&lua)
	// 		}
	// 	})
	// }

	pub(super) fn pdf_page_show(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (url, page, rect): (UrlRef, u16, Rect)| async move {
			if let Ok(area) = ADAPTOR.get().pdf_page_show(&url, page, *rect).await {
				Rect::from(area).into_lua(&lua)
			} else {
				Value::Nil.into_lua(&lua)
			}
		})
	}

	pub(super) fn pdf_page_precache(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|_, (src, page, dist): (UrlRef, u16, UrlRef)| async move {
			Ok(PdfRenderer::precache(dbg!(src.deref().deref()), dbg!(page), dbg!(dist.to_path_buf())).await.is_ok())
		})
	}
}
