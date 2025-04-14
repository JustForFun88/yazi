// use std::{process::{Child, Command, Stdio}, sync::{LazyLock, Mutex}};

// use anyhow::{Context, Result};
// use futures::future::join_all;
// use office_convert_client::{ConvertOffice, OfficeConvertClient,
// OfficeConvertLoadBalancer}; use tokio::{net::TcpStream, time::{Duration,
// sleep}};

// // Server ports to try connecting to
// const SERVER_PORTS: [u16; 2] = [8080, 8081];

// /// Global store of spawned server processes
// static SERVER_MANAGER: LazyLock<ServerManager> =
// LazyLock::new(ServerManager::new);

// /// Global load balancer instance with lazy initialization
// static LOAD_BALANCER: LazyLock<OfficeConvertLoadBalancer> = LazyLock::new(||
// { 	// Initialize in blocking mode since LazyLock doesn't support async
// directly 	tokio::runtime::Handle::current().block_on(async {
// 		// Create load balancer with whatever clients we successfully connected to
// 		// (Empty iterator is allowed by OfficeConvertLoadBalancer implementation)
// 		OfficeConvertLoadBalancer::new(try_connect_to_all_servers().await)
// 	})
// });

// /// Gets the global load balancer instance
// pub fn get_load_balancer() -> &'static OfficeConvertLoadBalancer {
// &LOAD_BALANCER }

// use std::path::Path;

// // use bytes::Bytes;
// use tokio::{fs, task};

// /// Converts a document to a PNG image with specified dimensions and page
// number pub async fn convert_to_png(
// 	path: &Path,
// 	width: u32,
// 	height: u32,
// 	page_number: u32,
// ) -> Result<Vec<u8>> {
// 	// Read the input file asynchronously
// 	let file_content =
// 		fs::read(path).await.with_context(|| format!("Failed to read file: {}",
// path.display()))?;

// 	// Perform all blocking operations in a single spawn_blocking
// 	let png_bytes = task::spawn_blocking(move || {
// 		let rt = tokio::runtime::Handle::current();

// 		// Get load balancer and perform conversion in the same blocking task
// 		let load_balancer = get_load_balancer();
// 		rt.block_on(load_balancer.convert(file_content))
// 	})
// 	.await
// 	.context("Conversion task failed")?
// 	.with_context(|| format!("Failed to convert document: {}",
// path.display()))?;

// 	Ok(png_bytes.to_vec())
// }

// async fn try_connect_to_all_servers() -> impl IntoIterator<Item =
// OfficeConvertClient> { 	let tasks = SERVER_PORTS
// 		.iter()
// 		.map(|&port| tokio::spawn(async move {
// try_connect_or_start_server(port).await.ok() }));

// 	join_all(tasks).await.into_iter().filter_map(|res| res.ok().flatten())
// }

// async fn try_connect_or_start_server(port: u16) ->
// Result<OfficeConvertClient> { 	// 1. First try to connect to existing server
// 	if let Ok(client) = try_connect_with_timeout(port,
// Duration::from_secs(1)).await { 		return Ok(client);
// 	}

// 	// 2. If connection failed - start the server
// 	start_server(port).context("Failed to start server")?;

// 	// 3. Wait until server becomes available
// 	wait_for_server(port, Duration::from_secs(5), Duration::from_millis(500))
// 		.await
// 		.context("Server didn't become ready in time")?;

// 	// 4. Try connecting again
// 	try_connect_with_timeout(port, Duration::from_secs(2))
// 		.await
// 		.context("Failed to connect even after server start")
// }

// async fn try_connect_with_timeout(port: u16, timeout: Duration) ->
// Result<OfficeConvertClient> { 	tokio::time::timeout(timeout, async {
// 		// Check TCP port availability
// 		TcpStream::connect(("localhost", port)).await?;

// 		// If port is available, create HTTP client
// 		OfficeConvertClient::new(format!("http://localhost:{port}")).context("Failed to create client")
// 	})
// 	.await
// 	.context("Connection timeout")?
// }

// /// Starts the server and stores its process
// fn start_server(port: u16) -> Result<()> {
// 	let child = Command::new("office-convert-server")
// 		.arg("--port")
// 		.arg(port.to_string())
// 		.arg("--office-path")
// 		.arg("C:/Program Files/LibreOffice/program")
// 		.stdout(Stdio::null())
// 		.stderr(Stdio::null())
// 		.spawn()
// 		.context("Failed to execute server process")?;

// 	let _ = SERVER_MANAGER.add_process(child).map_err(|mut child| {
// 		let _ = child.kill().and_then(|_| child.wait());
// 	});
// 	Ok(())
// }

// async fn wait_for_server(port: u16, max_wait: Duration, retry_interval:
// Duration) -> Result<()> { 	let start = tokio::time::Instant::now();

// 	loop {
// 		if TcpStream::connect(("localhost", port)).await.is_ok() {
// 			return Ok(());
// 		}

// 		if start.elapsed() > max_wait {
// 			return Err(anyhow::anyhow!("Timeout reached"));
// 		}

// 		sleep(retry_interval).await;
// 	}
// }

// struct ServerManager {
// 	processes: Mutex<Vec<Child>>,
// }

// impl ServerManager {
// 	fn new() -> Self { ServerManager { processes: Mutex::new(Vec::new()) } }

// 	fn add_process(&self, child: Child) -> Result<(), Child> {
// 		match self.processes.lock() {
// 			Ok(mut guard) => guard.push(child),
// 			Err(_) => return Err(child),
// 		}
// 		Ok(())
// 	}

// 	fn terminate_all(&self) -> Result<()> {
// 		let mut processes = self.processes.lock().map_err(|_|
// anyhow::anyhow!("Mutex poison error"))?;

// 		for mut child in processes.drain(..) {
// 			let _ = child.kill().and_then(|_| child.wait());
// 		}

// 		Ok(())
// 	}
// }

// impl Drop for ServerManager {
// 	fn drop(&mut self) { let _ = self.terminate_all(); }
// }

// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================

use std::{path::{Path, PathBuf}, sync::{atomic::{AtomicUsize, Ordering}, mpsc::{self, Receiver, RecvTimeoutError, Sender}}, thread::{self, JoinHandle}, time::Duration};

use anyhow::{Context, Result, anyhow};
use libreofficekit::{DocUrl, Office, OfficeError};
use rand::{Rng, distr::Alphanumeric};
use tokio::{sync::oneshot, time::timeout};
use twox_hash::XxHash3_128;
use yazi_config::YAZI;

/// Пул worker'ов для конвертации документов
pub struct OfficePool {
	workers:     Vec<WorkerHandle>,
	next_worker: AtomicUsize,
}

/// Обработчик отдельного worker'а
struct WorkerHandle {
	task_sender:   Sender<ConversionTask>,
	thread_handle: Option<JoinHandle<()>>,
}

/// Задача на конвертацию
struct ConversionTask {
	input_path:    PathBuf,
	result_sender: oneshot::Sender<Result<Vec<u8>>>,
}

impl OfficePool {
	/// Создает новый пул worker'ов
	pub fn new(pool_size: usize, office_path: impl AsRef<Path>) -> Result<Self> {
		let mut workers = Vec::with_capacity(pool_size);

		for i in 0..pool_size {
			workers.push(WorkerHandle::new(office_path.as_ref(), i)?);
		}

		Ok(Self { workers, next_worker: AtomicUsize::new(0) })
	}

	/// Конвертирует документ (балансировка round-robin)
	pub fn convert(&self, input_path: impl AsRef<Path>) -> Result<Vec<u8>> {
		let (result_sender, result_receiver) = oneshot::channel();

		// Round-robin выбор worker'а
		let idx = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
		let worker = &self.workers[idx];

		worker
			.task_sender
			.send(ConversionTask { input_path: input_path.as_ref().to_path_buf(), result_sender })?;

		// Блокируем текущий поток до получения результата
		result_receiver.blocking_recv()?
	}

	pub async fn convert2(&self, input_path: impl AsRef<Path>) -> Result<Vec<u8>> {
		let (result_sender, result_receiver) = oneshot::channel();

		// Round-robin выбор worker'а
		let idx = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
		let worker = &self.workers[idx];

		worker
			.task_sender
			.send(ConversionTask { input_path: input_path.as_ref().to_path_buf(), result_sender })?;

		match timeout(Duration::from_secs(30), result_receiver).await {
			Ok(Ok(res)) => res,
			Ok(Err(_)) => Err(anyhow!("oneshot canceled")),
			Err(_) => Err(anyhow!("Conversion timeout")),
		}
	}
}

impl WorkerHandle {
	fn new(office_path: &Path, worker_id: usize) -> Result<Self> {
		let (task_sender, task_receiver) = mpsc::channel();
		let (init_sender, init_receiver) = mpsc::channel();

		let office_path = office_path.to_path_buf();
		let thread_handle = thread::spawn(move || {
			Self::run_worker(office_path, task_receiver, worker_id, init_sender)
				.unwrap_or_else(|e| eprintln!("Worker {worker_id} failed: {e:?}"));
		});

		match init_receiver.recv_timeout(Duration::from_secs(30)) {
			Ok(Ok(())) => Ok(Self { task_sender, thread_handle: Some(thread_handle) }),
			Ok(Err(e)) => {
				let _ = thread_handle.join();
				Err(e)
			}
			Err(e) => {
				let join_err = thread_handle.join().err();
				Err(match e {
					RecvTimeoutError::Timeout => anyhow!("Worker initialization timeout"),
					RecvTimeoutError::Disconnected => anyhow!("Worker disconnected"),
				})
				.context(
					join_err
						.map(|e| anyhow!("Thread panic: {:?}", e))
						.unwrap_or_else(|| anyhow!("Worker thread failed during initialization")),
				)
			}
		}
	}

	fn run_worker(
		office_path: PathBuf,
		task_receiver: Receiver<ConversionTask>,
		worker_id: usize,
		init_sender: mpsc::Sender<Result<()>>,
	) -> Result<()> {
		let office = match Office::new(&office_path) {
			Ok(office) => office,
			Err(e) => {
				let _ = init_sender.send(Err(anyhow!("Failed to initialize LibreOffice: {e}")));
				return Err(anyhow!("Failed to initialize LibreOffice: {e}"));
			}
		};

		// Generate random ID for the path name
		let temp_output = TempFile::new();

		// Инициализация прошла успешно
		let _ = init_sender.send(Ok(()));

		for task in task_receiver {
			let result = Self::process_task(&office, task.input_path, &temp_output);
			if task.result_sender.send(result).is_err() {
				eprintln!("Worker {worker_id}: failed to send result");
			}
		}

		Ok(())
	}

	fn process_task(office: &Office, input_path: PathBuf, temp_output: &TempFile) -> Result<Vec<u8>> {
		let input_url = DocUrl::from_path(&input_path)?;
		let mut doc =
			match office.document_load_with_options(&input_url, "InteractionHandler=0,Batch=1") {
				Ok(doc) => doc,
				Err(e) => return Err(e.into()),
			};

		// Convert to PDF
		if !doc.save_as(&DocUrl::try_from(temp_output)?, "pdf", None)? {
			return Err(anyhow!("Failed to convert document to PDF"));
		}

		std::fs::read(temp_output).context("Failed to read converted PDF file")
	}
}

impl Drop for WorkerHandle {
	fn drop(&mut self) {
		// // Закрываем канал (это завершит цикл в потоке)
		// drop(self.task_sender.take());

		// Дожидаемся завершения потоков
		if let Some(handle) = self.thread_handle.take() {
			let _ = handle.join();
		}
	}
}

pub(crate) struct TempFile {
	path: PathBuf,
}

impl TempFile {
	pub(crate) fn new() -> Self {
		let random_id = rand::rng()
			.sample_iter(&Alphanumeric)
			.take(10)
			.map(|value| value as char)
			.collect::<String>();

		TempFile { path: std::env::temp_dir().join(format!("lo_output_{random_id}.pdf")) }
	}

	pub(crate) fn from_path(file_path: &Path) -> Self {
		let hex = {
			let mut hasher = XxHash3_128::new();
			hasher.write(file_path.as_os_str().as_encoded_bytes());
			hasher.write(format!("//{:?}", std::time::Instant::now()).as_bytes());
			format!("{:x}", hasher.finish_128())
		};

		TempFile { path: YAZI.preview.cache_dir.join(hex) }
	}
}

impl Drop for TempFile {
	fn drop(&mut self) {
		if self.path.exists() {
			let _ = std::fs::remove_file(&self.path);
		}
	}
}

impl AsRef<Path> for TempFile {
	fn as_ref(&self) -> &Path { &self.path }
}

impl TryFrom<&TempFile> for DocUrl {
	type Error = OfficeError;

	fn try_from(value: &TempFile) -> std::result::Result<Self, Self::Error> {
		DocUrl::from_path(&value.path)
	}
}

// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================
// ====== ============================================================================================================================

// use std::{env::temp_dir, ffi::CStr, path::PathBuf, rc::Rc, sync::Arc};

// use anyhow::{Context, anyhow};
// use axum::{Extension, Json, Router, body::Body, extract::DefaultBodyLimit,
// http::{HeaderValue, Response, StatusCode, header}, routing::{get, post}}; use
// axum_typed_multipart::{FieldData, TryFromMultipart, TypedMultipart};
// use bytes::Bytes;
// use clap::Parser;
// use error::DynHttpError;
// use libreofficekit::{CallbackType, DocUrl, FilterTypes, Office, OfficeError,
// OfficeOptionalFeatures, OfficeVersionInfo}; use parking_lot::Mutex;
// use rand::{Rng, distributions::Alphanumeric};
// use serde::Serialize;
// use tokio::sync::{mpsc, oneshot};
// use tracing::{debug, error};
// use tracing_subscriber::EnvFilter;

// mod error;

// #[derive(Parser, Debug)]
// #[command(version, about, long_about = None)]
// struct Args {
// 	/// Path to the office installation (Omit to determine automatically)
// 	#[arg(long)]
// 	office_path: Option<String>,

// 	/// Port to bind the server to, defaults to 8080
// 	#[arg(long)]
// 	port: Option<u16>,

// 	/// Host to bind the server to, defaults to 0.0.0.0
// 	#[arg(long)]
// 	host: Option<String>,
// }

// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
// 	_ = dotenvy::dotenv();

// 	// Start configuring a `fmt` subscriber
// 	let subscriber = tracing_subscriber::fmt()
//         // Use the logging options from env variables
//         .with_env_filter(EnvFilter::from_default_env())
//         // Display source code file paths
//         .with_file(true)
//         // Display source code line numbers
//         .with_line_number(true)
//         // Don't display the event's target (module path)
//         .with_target(false)
//         // Build the subscriber
//         .finish();

// 	// use that subscriber to process traces emitted after this point
// 	tracing::subscriber::set_global_default(subscriber)?;

// 	let args = Args::parse();

// 	let mut office_path: Option<PathBuf> = None;

// 	// Try loading office path from command line
// 	if let Some(path) = args.office_path {
// 		office_path = Some(PathBuf::from(&path));
// 	}

// 	// Try loading office path from environment variables
// 	if office_path.is_none() {
// 		if let Ok(path) = std::env::var("LIBREOFFICE_SDK_PATH") {
// 			office_path = Some(PathBuf::from(&path));
// 		}
// 	}

// 	// Try determine default office path
// 	if office_path.is_none() {
// 		office_path = Office::find_install_path();
// 	}

// 	// Check a path was provided
// 	let office_path = match office_path {
// 		Some(value) => value,
// 		None => {
// 			error!("no office install path provided, cannot start server");
// 			panic!();
// 		}
// 	};

// 	debug!("using libreoffice install from: {}", office_path.display());

// 	// Determine the address to run the server on
// 	let server_address = if args.host.is_some() || args.port.is_some() {
// 		let host = args.host.unwrap_or_else(|| "0.0.0.0".to_string());
// 		let port = args.port.unwrap_or(8080);

// 		format!("{host}:{port}")
// 	} else {
// 		std::env::var("SERVER_ADDRESS").context("missing SERVER_ADDRESS")?
// 	};

// 	// Create office access and get office details
// 	let (office_details, office_handle) =
// create_office_runner(office_path).await?;

// 	// Create the router
// 	let app = Router::new()
// 		.route("/status", get(status))
// 		.route("/office-version", get(office_version))
// 		.route("/supported-formats", get(supported_formats))
// 		.route("/convert", post(convert))
// 		.route("/collect-garbage", post(collect_garbage))
// 		.layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
// 		.layer(Extension(office_handle))
// 		.layer(Extension(Arc::new(office_details)));

// 	// Create a TCP listener
// 	let listener =
// 		tokio::net::TcpListener::bind(&server_address).await.context("failed to
// bind http server")?;

// 	debug!("server started on: {server_address}");

// 	// Serve the app from the listener
// 	axum::serve(listener, app).await.context("failed to serve")?;

// 	Ok(())
// }

// /// Messages the office runner can process
// pub enum OfficeMsg {
// 	/// Message to convert a file
// 	Convert {
// 		/// The file bytes to convert
// 		bytes: Bytes,

// 		/// The return channel for sending back the result
// 		tx: oneshot::Sender<anyhow::Result<Bytes>>,
// 	},

// 	/// Tells office to clean up and trim its memory usage
// 	CollectGarbage,

// 	/// Message to check if the server is busy, ignored
// 	BusyCheck,
// }

// /// Handle to send messages to the office runner
// #[derive(Clone)]
// pub struct OfficeHandle(mpsc::Sender<OfficeMsg>);

// /// Creates a new office runner on its own thread providing
// /// a handle to access it via messages
// async fn create_office_runner(path: PathBuf) ->
// anyhow::Result<(OfficeDetails, OfficeHandle)> { 	let (tx, rx) =
// mpsc::channel(1);

// 	let (startup_tx, startup_rx) = oneshot::channel();

// 	std::thread::spawn(move || {
// 		let mut startup_tx = Some(startup_tx);

// 		if let Err(cause) = office_runner(path, rx, &mut startup_tx) {
// 			error!(%cause, "failed to start office runner");

// 			// Send the error to the startup channel if its still available
// 			if let Some(startup_tx) = startup_tx.take() {
// 				_ = startup_tx.send(Err(cause));
// 			}
// 		}
// 	});

// 	// Wait for a successful startup
// 	let office_details = startup_rx.await.context(
// 		"startup channel unavailable",
// 	)??;
// 	let office_handle = OfficeHandle(tx);

// 	Ok((office_details, office_handle))
// }

// #[derive(Debug, Default)]
// struct RunnerState {
// 	password_requested: bool,
// }

// #[derive(Debug)]
// struct OfficeDetails {
// 	filter_types: Option<FilterTypes>,
// 	version:      Option<OfficeVersionInfo>,
// }

// /// Main event loop for an office runner
// fn office_runner(
// 	path: PathBuf,
// 	mut rx: mpsc::Receiver<OfficeMsg>,
// 	startup_tx: &mut Option<oneshot::Sender<anyhow::Result<OfficeDetails>>>,
// ) -> anyhow::Result<()> {
// 	// Create office instance
// 	let office = Office::new(&path).context(
// 		"failed to create office instance",
// 	)?;

// 	let tmp_dir = temp_dir();

// 	// Generate random ID for the path name
// 	let random_id = rand::thread_rng()
// 		.sample_iter(&Alphanumeric)
// 		.take(10)
// 		.map(|value| value as char)
// 		.collect::<String>();

// 	// Create input and output paths
// 	let temp_in = tmp_dir.join(format!("lo_native_input_{random_id}"));
// 	let temp_out = tmp_dir.join(format!("lo_native_output_{random_id}.pdf"));

// 	let runner_state = Rc::new(Mutex::new(RunnerState::default()));

// 	// Allow prompting for passwords
// 	office
// 		.set_optional_features(OfficeOptionalFeatures::DOCUMENT_PASSWORD)
// 		.context("failed to set optional features")?;

// 	// Load supported filters and office version details
// 	let filter_types = office.get_filter_types().ok();
// 	let version = office.get_version_info().ok();

// 	office
// 		.register_callback({
// 			let runner_state = runner_state.clone();
// 			let input_url = DocUrl::from_path(&temp_in).context(
// 				"failed to create input url",
// 			)?;

// 			move |office, ty, payload| {
// 				debug!(?ty, "callback invoked");

// 				let state = &mut *runner_state.lock();

// 				if let CallbackType::DocumentPassword = ty {
// 					state.password_requested = true;

// 					// Provide now password
// 					if let Err(cause) = office.set_document_password(&input_url, None) {
// 						error!(?cause, "failed to set document password");
// 					}
// 				}

// 				if let CallbackType::JSDialog = ty {
// 					let payload = unsafe { CStr::from_ptr(payload) };
// 					let value: serde_json::Value =
// serde_json::from_slice(payload.to_bytes()).unwrap();

// 					debug!(?value, "js dialog request");
// 				}
// 			}
// 		})
// 		.context("failed to register office callback")?;

// 	// Report successful startup
// 	if let Some(startup_tx) = startup_tx.take() {
// 		_ = startup_tx.send(Ok(OfficeDetails { filter_types, version }));
// 	}

// 	// Get next message
// 	while let Some(msg) = rx.blocking_recv() {
// 		let (input, output) = match msg {
// 			OfficeMsg::Convert { bytes, tx } => (bytes, tx),

// 			OfficeMsg::CollectGarbage => {
// 				if let Err(cause) = office.trim_memory(2000) {
// 					error!(%cause, "failed to collect garbage")
// 				}
// 				continue;
// 			}
// 			// Busy checks are ignored
// 			OfficeMsg::BusyCheck => continue,
// 		};

// 		let temp_in = TempFile { path: temp_in.clone() };
// 		let temp_out = TempFile { path: temp_out.clone() };

// 		// Convert document
// 		let result = convert_document(&office, temp_in, temp_out, input,
// &runner_state);

// 		// Send response
// 		_ = output.send(result);

// 		// Reset runner state
// 		*runner_state.lock() = RunnerState::default();
// 	}

// 	Ok(())
// }

// /// Converts the provided document bytes into PDF format returning
// /// the converted bytes
// fn convert_document(
// 	office: &Office,

// 	temp_in: TempFile,
// 	temp_out: TempFile,

// 	input: Bytes,

// 	runner_state: &Rc<Mutex<RunnerState>>,
// ) -> anyhow::Result<Bytes> {
// 	let in_url = temp_in.doc_url()?;
// 	let out_url = temp_out.doc_url()?;

// 	// Write to temp file
// 	std::fs::write(&temp_in.path, input).context(
// 		"failed to write temp input",
// 	)?;

// 	// Load document
// 	let mut doc = match office.document_load_with_options(&in_url,
// "InteractionHandler=0,Batch=1") { 		Ok(value) => value,
// 		Err(err) => match err {
// 			OfficeError::OfficeError(err) => {
// 				error!(%err, "failed to load document");

// 				let _state = &*runner_state.lock();

// 				// File was encrypted with a password
// 				if err.contains("Unsupported URL") {
// 					return Err(anyhow!("file is encrypted"));
// 				}

// 				// File is malformed or corrupted
// 				if err.contains(
// 					"loadComponentFromURL returned an empty reference",
// 				) {
// 					return Err(anyhow!("file is corrupted"));
// 				}

// 				return Err(OfficeError::OfficeError(err).into());
// 			}
// 			err => return Err(err.into()),
// 		},
// 	};

// 	debug!("document loaded");

// 	// Convert document
// 	let result = doc.save_as(&out_url, "pdf", None)?;

// 	// Attempt to free up some memory
// 	_ = office.trim_memory(1000);

// 	if !result {
// 		return Err(anyhow!("failed to convert file"));
// 	}

// 	// Read document context
// 	let bytes = std::fs::read(&temp_out.path).context(
// 		"failed to read temp out file",
// 	)?;

// 	Ok(Bytes::from(bytes))
// }

// /// Request to convert a file
// #[derive(TryFromMultipart)]
// struct UploadAssetRequest {
// 	/// The file to convert
// 	#[form_data(limit = "unlimited")]
// 	file: FieldData<Bytes>,
// }

// /// POST /convert
// ///
// /// Converts the provided file to PDF format responding with the PDF file
// async fn convert(
// 	Extension(office): Extension<OfficeHandle>,
// 	TypedMultipart(UploadAssetRequest { file }):
// TypedMultipart<UploadAssetRequest>, ) -> Result<Response<Body>, DynHttpError>
// { 	let (tx, rx) = oneshot::channel();

// 	// Convert the file
// 	office
// 		.0
// 		.send(OfficeMsg::Convert { bytes: file.contents, tx })
// 		.await
// 		.context("failed to send convert request")?;

// 	// Wait for the response
// 	let converted = rx.await.context("failed to get convert response")??;

// 	// Build the response
// 	let response = Response::builder()
// 		.header(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"))
// 		.body(Body::from(converted))
// 		.context("failed to create response")?;

// 	Ok(response)
// }

// /// Result from checking the server busy state
// #[derive(Serialize)]
// struct StatusResponse {
// 	/// Whether the server is busy
// 	is_busy: bool,
// }

// /// GET /status
// ///
// /// Checks if the converter is currently busy
// async fn status(Extension(office): Extension<OfficeHandle>) ->
// Json<StatusResponse> { 	let is_locked =
// office.0.try_send(OfficeMsg::BusyCheck).is_err(); 	Json(StatusResponse {
// is_busy: is_locked }) }

// #[derive(Serialize)]
// struct VersionResponse {
// 	/// Major version of LibreOffice
// 	major:    u32,
// 	/// Minor version of LibreOffice
// 	minor:    u32,
// 	/// Libreoffice "Build ID"
// 	build_id: String,
// }

// /// GET /office-version
// ///
// /// Checks if the converter is currently busy
// async fn office_version(
// 	Extension(details): Extension<Arc<OfficeDetails>>,
// ) -> Result<Json<VersionResponse>, StatusCode> {
// 	let version = details.version.as_ref().ok_or(StatusCode::NOT_FOUND)?;
// 	let product_version = &version.product_version;

// 	Ok(Json(VersionResponse {
// 		build_id: version.build_id.clone(),
// 		major:    product_version.major,
// 		minor:    product_version.minor,
// 	}))
// }

// #[derive(Serialize)]
// struct SupportedFormat {
// 	/// Name of the file format
// 	name: String,
// 	/// Mime type of the format
// 	mime: String,
// }

// /// GET /supported-formats
// ///
// /// Provides an array of supported file formats
// async fn supported_formats(
// 	Extension(details): Extension<Arc<OfficeDetails>>,
// ) -> Result<Json<Vec<SupportedFormat>>, StatusCode> {
// 	let types = details.filter_types.as_ref().ok_or(StatusCode::NOT_FOUND)?;

// 	let formats: Vec<SupportedFormat> = types
// 		.values
// 		.iter()
// 		.map(|(key, value)| SupportedFormat {
// 			name: key.to_string(),
// 			mime: value.media_type.to_string(),
// 		})
// 		.collect();

// 	Ok(Json(formats))
// }

// /// POST /collect-garbage
// ///
// /// Collects garbage from the office converter
// async fn collect_garbage(Extension(office): Extension<OfficeHandle>) ->
// StatusCode { 	_ = office.0.send(OfficeMsg::CollectGarbage).await;
// 	StatusCode::OK
// }
