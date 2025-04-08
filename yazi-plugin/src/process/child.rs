use std::{ops::DerefMut, time::Duration};

use futures::future::try_join3;
use mlua::{AnyUserData, ExternalError, IntoLua, IntoLuaMulti, Table, UserData, Value};
use tokio::{io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter}, process::{ChildStderr, ChildStdin, ChildStdout}, select};
use yazi_binding::Error;

use super::Status;
use crate::process::Output;

pub struct Child {
	inner:  tokio::process::Child,
	stdin:  Option<BufWriter<ChildStdin>>,
	stdout: Option<BufReader<ChildStdout>>,
	stderr: Option<BufReader<ChildStderr>>,
}

impl Child {
	pub fn new(mut inner: tokio::process::Child) -> Self {
		let stdin = inner.stdin.take().map(BufWriter::new);
		let stdout = inner.stdout.take().map(BufReader::new);
		let stderr = inner.stderr.take().map(BufReader::new);
		Self { inner, stdin, stdout, stderr }
	}
}

impl UserData for Child {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		#[inline]
		async fn read_line(me: &mut Child) -> (Option<Vec<u8>>, u8) {
			async fn read(r: Option<impl AsyncBufReadExt + Unpin>) -> Option<Vec<u8>> {
				let mut buf = Vec::new();
				match r?.read_until(b'\n', &mut buf).await {
					Ok(0) | Err(_) => None,
					Ok(_) => Some(buf),
				}
			}

			select! {
				r @ Some(_) = read(me.stdout.as_mut()) => (r, 0u8),
				r @ Some(_) = read(me.stderr.as_mut()) => (r, 1u8),
				else => (None, 2u8),
			}
		}

		methods.add_async_method_mut("read", |_, mut me, len: usize| async move {
			async fn read(r: Option<impl AsyncBufReadExt + Unpin>, len: usize) -> Option<Vec<u8>> {
				let mut r = r?;
				let mut buf = vec![0; len];
				match r.read(&mut buf).await {
					Ok(0) | Err(_) => return None,
					Ok(n) => buf.truncate(n),
				}
				Some(buf)
			}

			let me = me.deref_mut();
			Ok(select! {
				Some(r) = read(me.stdout.as_mut(), len) => (r, 0u8),
				Some(r) = read(me.stderr.as_mut(), len) => (r, 1u8),
				else => (vec![], 2u8)
			})
		});
		methods.add_async_method_mut("read_line", |lua, mut me, ()| async move {
			match read_line(&mut me).await {
				(Some(b), event) => (lua.create_string(b)?, event).into_lua_multi(&lua),
				(None, event) => (Value::Nil, event).into_lua_multi(&lua),
			}
		});
		methods.add_async_method_mut("read_line_with", |lua, mut me, options: Table| async move {
			let timeout = Duration::from_millis(options.raw_get("timeout")?);
			let Ok(result) = tokio::time::timeout(timeout, read_line(&mut me)).await else {
				return (Value::Nil, 3u8).into_lua_multi(&lua);
			};
			match result {
				(Some(b), event) => (lua.create_string(b)?, event).into_lua_multi(&lua),
				(None, event) => (Value::Nil, event).into_lua_multi(&lua),
			}
		});

		methods.add_async_method_mut("write_all", |lua, mut me, src: mlua::String| async move {
			let Some(stdin) = &mut me.stdin else {
				return Err("stdin is not piped".into_lua_err());
			};
			match stdin.write_all(&src.as_bytes()).await {
				Ok(()) => (true, Value::Nil).into_lua_multi(&lua),
				Err(e) => (false, Error::Io(e)).into_lua_multi(&lua),
			}
		});
		methods.add_async_method_mut("flush", |lua, mut me, ()| async move {
			let Some(stdin) = &mut me.stdin else {
				return Err("stdin is not piped".into_lua_err());
			};
			match stdin.flush().await {
				Ok(()) => (true, Value::Nil).into_lua_multi(&lua),
				Err(e) => (false, Error::Io(e)).into_lua_multi(&lua),
			}
		});

		methods.add_async_method_mut("wait", |lua, mut me, ()| async move {
			drop(me.stdin.take());
			match me.inner.wait().await {
				Ok(status) => (Status::new(status), Value::Nil).into_lua_multi(&lua),
				Err(e) => (Value::Nil, Error::Io(e)).into_lua_multi(&lua),
			}
		});
		methods.add_async_function("wait_with_output", |lua, ud: AnyUserData| async move {
			async fn read_to_end(r: &mut Option<impl AsyncBufReadExt + Unpin>) -> io::Result<Vec<u8>> {
				let mut vec = Vec::new();
				if let Some(r) = r.as_mut() {
					r.read_to_end(&mut vec).await?;
				}
				Ok(vec)
			}

			let mut me = ud.take::<Self>()?;
			let mut stdout_pipe = me.stdout.take();
			let mut stderr_pipe = me.stderr.take();

			let stdout_fut = read_to_end(&mut stdout_pipe);
			let stderr_fut = read_to_end(&mut stderr_pipe);

			drop(me.stdin.take());
			let result = try_join3(me.inner.wait(), stdout_fut, stderr_fut).await;
			drop(stdout_pipe);
			drop(stderr_pipe);

			match result {
				Ok((status, stdout, stderr)) => {
					(Output::new(std::process::Output { status, stdout, stderr }), Value::Nil)
						.into_lua_multi(&lua)
				}
				Err(e) => (Value::Nil, Error::Io(e)).into_lua_multi(&lua),
			}
		});
		methods.add_method_mut("start_kill", |lua, me, ()| match me.inner.start_kill() {
			Ok(_) => (true, Value::Nil).into_lua_multi(lua),
			Err(e) => (false, Error::Io(e)).into_lua_multi(lua),
		});

		methods.add_method_mut("take_stdin", |lua, me, ()| match me.stdin.take() {
			Some(stdin) => lua.create_any_userdata(stdin.into_inner())?.into_lua(lua),
			None => Ok(Value::Nil),
		});
		methods.add_method_mut("take_stdout", |lua, me, ()| match me.stdout.take() {
			Some(stdout) => lua.create_any_userdata(stdout.into_inner())?.into_lua(lua),
			None => Ok(Value::Nil),
		});
		methods.add_method_mut("take_stderr", |lua, me, ()| match me.stderr.take() {
			Some(stderr) => lua.create_any_userdata(stderr.into_inner())?.into_lua(lua),
			None => Ok(Value::Nil),
		});
	}
}
