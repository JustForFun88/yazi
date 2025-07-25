use mlua::{ExternalError, IntoLua, Lua, Value};
use yazi_shared::event::CmdCow;

#[derive(Debug)]
pub struct VisualModeOpt {
	pub unset: bool,
}

impl From<CmdCow> for VisualModeOpt {
	fn from(c: CmdCow) -> Self { Self { unset: c.bool("unset") } }
}

impl IntoLua for &VisualModeOpt {
	fn into_lua(self, _: &Lua) -> mlua::Result<Value> { Err("unsupported".into_lua_err()) }
}
