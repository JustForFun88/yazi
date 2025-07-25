use anyhow::Context;
use futures::future::join_all;
use mlua::{ExternalError, ExternalResult, Function, IntoLuaMulti, Lua, MultiValue, Value, Variadic};
use tokio::sync::oneshot;
use yazi_binding::{runtime, runtime_mut};
use yazi_dds::Sendable;
use yazi_parser::app::{PluginCallback, PluginOpt};
use yazi_proxy::AppProxy;
use yazi_shared::event::Data;

use super::Utils;
use crate::{bindings::{MpscRx, MpscTx, MpscUnboundedRx, MpscUnboundedTx, OneshotRx, OneshotTx}, loader::LOADER};

impl Utils {
	pub(super) fn sync(lua: &Lua, isolate: bool) -> mlua::Result<Function> {
		if isolate {
			lua.create_function(|lua, ()| {
				let Some(block) = runtime_mut!(lua)?.next_block() else {
					return Err("`ya.sync()` must be called in a plugin").into_lua_err();
				};

				lua.create_async_function(move |lua, args: MultiValue| async move {
					let Some(cur) = runtime!(lua)?.current_owned() else {
						return Err("`ya.sync()` block must be used within a plugin").into_lua_err();
					};

					Self::retrieve(&lua, &cur, block, args)
						.await
						.and_then(|data| Sendable::list_to_values(&lua, data))
						.with_context(|| format!("Failed to execute sync block-{block} in `{cur}` plugin"))
						.into_lua_err()
				})
			})
		} else {
			lua.create_function(|lua, f: Function| {
				let mut rt = runtime_mut!(lua)?;
				if !rt.put_block(f.clone()) {
					return Err("`ya.sync()` must be called in a plugin").into_lua_err();
				}

				let cur = rt.current_owned().unwrap();
				lua.create_function(move |lua, mut args: MultiValue| {
					args.push_front(Value::Table(LOADER.try_load(lua, &cur)?));
					f.call::<MultiValue>(args)
				})
			})
		}
	}

	pub(super) fn chan(lua: &Lua) -> mlua::Result<Function> {
		lua.create_function(|lua, (r#type, buffer): (mlua::String, Option<usize>)| {
			match (&*r#type.as_bytes(), buffer) {
				(b"mpsc", Some(buffer)) if buffer < 1 => {
					Err("Buffer size must be greater than 0".into_lua_err())
				}
				(b"mpsc", Some(buffer)) => {
					let (tx, rx) = tokio::sync::mpsc::channel::<Value>(buffer);
					(MpscTx(tx), MpscRx(rx)).into_lua_multi(lua)
				}
				(b"mpsc", None) => {
					let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
					(MpscUnboundedTx(tx), MpscUnboundedRx(rx)).into_lua_multi(lua)
				}
				(b"oneshot", _) => {
					let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
					(OneshotTx(Some(tx)), OneshotRx(Some(rx))).into_lua_multi(lua)
				}
				_ => Err("Channel type must be `mpsc` or `oneshot`".into_lua_err()),
			}
		})
	}

	pub(super) fn join(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|_, fns: Variadic<Function>| async move {
			let mut results = MultiValue::with_capacity(fns.len());
			for r in join_all(fns.into_iter().map(|f| f.call_async::<MultiValue>(()))).await {
				results.extend(r?);
			}
			Ok(results)
		})
	}

	// TODO
	pub(super) fn select(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|_lua, _futs: MultiValue| async move { Ok(()) })
	}

	async fn retrieve(
		lua: &Lua,
		id: &str,
		calls: usize,
		args: MultiValue,
	) -> mlua::Result<Vec<Data>> {
		let args = Sendable::values_to_list(lua, args)?;
		let (tx, rx) = oneshot::channel::<Vec<Data>>();

		let callback: PluginCallback = {
			let id = id.to_owned();
			Box::new(move |lua, plugin| {
				let Some(block) = runtime!(lua)?.get_block(&id, calls) else {
					return Err("sync block not found".into_lua_err());
				};

				let args = [Ok(Value::Table(plugin))]
					.into_iter()
					.chain(args.into_iter().map(|d| Sendable::data_to_value(lua, d)))
					.collect::<mlua::Result<MultiValue>>()?;

				let values = Sendable::values_to_list(lua, block.call(args)?)?;
				tx.send(values).map_err(|_| "send failed".into_lua_err())
			})
		};

		AppProxy::plugin(PluginOpt::new_callback(id.to_owned(), callback));

		rx.await.into_lua_err()
	}
}
