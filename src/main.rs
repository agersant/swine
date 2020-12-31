#![cfg_attr(all(windows, feature = "ui"), windows_subsystem = "windows")]
#![recursion_limit = "256"]

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use anyhow::*;
use log::info;
use simplelog::{CombinedLogger, LevelFilter, TermLogger, TerminalMode, WriteLogger};
use std::fs;
use std::path::PathBuf;

mod app;
mod db;
mod options;
mod service;
#[cfg(test)]
mod test;
mod ui;
mod utils;

#[cfg(unix)]
fn daemonize(foreground: bool, pid_file_path: &Option<std::path::PathBuf>) -> Result<()> {
	use unix_daemonize::{daemonize_redirect, ChdirMode};

	if foreground {
		return Ok(());
	}

	// TODO fix me

	let log_path = log_file_path.clone().unwrap_or_else(|| {
		let mut path = PathBuf::from(option_env!("POLARIS_LOG_DIR").unwrap_or("."));
		path.push("polaris.log");
		path
	});
	fs::create_dir_all(&log_path.parent().unwrap())?;

	let pid = match daemonize_redirect(Some(&log_path), Some(&log_path), ChdirMode::NoChdir) {
		Ok(p) => p,
		Err(e) => bail!("Daemonize error: {:#?}", e),
	};

	let pid_path = pid_file_path.clone().unwrap_or_else(|| {
		let mut path = PathBuf::from(option_env!("POLARIS_PID_DIR").unwrap_or("."));
		path.push("polaris.pid");
		path
	});
	fs::create_dir_all(&pid_path.parent().unwrap())?;

	let mut file = fs::File::create(pid_path)?;
	file.write_all(pid.to_string().as_bytes())?;
	Ok(())
}

#[cfg(unix)]
fn notify_ready() {
	if let Ok(true) = sd_notify::booted() {
		if let Err(e) = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]) {
			error!("Unable to send ready notification: {}", e);
		}
	}
}

#[cfg(not(unix))]
fn notify_ready() {}

fn init_logging(log_level: LevelFilter, log_file_path: &PathBuf) -> Result<()> {
	let log_config = simplelog::ConfigBuilder::new()
		.set_location_level(LevelFilter::Error)
		.build();

	CombinedLogger::init(vec![
		TermLogger::new(log_level, log_config.clone(), TerminalMode::Mixed),
		WriteLogger::new(
			log_level,
			log_config.clone(),
			fs::File::create(log_file_path)?,
		),
	])?;

	Ok(())
}

fn main() -> Result<()> {
	// Parse CLI options
	let args: Vec<String> = std::env::args().collect();
	let options_manager = options::Manager::new();
	let cli_options = options_manager.parse(&args[1..])?;

	if cli_options.show_help {
		let program = args[0].clone();
		let brief = format!("Usage: {} [options]", program);
		print!("{}", options_manager.usage(&brief));
		return Ok(());
	}

	#[cfg(unix)]
	daemonize(
		cli_options.foreground,
		&cli_options.pid_file_path,
		&cli_options.log_file_path,
	)?;

	// Logging
	let log_level = cli_options.log_level.unwrap_or(LevelFilter::Info);
	// TODO default path on windows
	let log_file_path = cli_options.log_file_path.clone().unwrap_or(
		PathBuf::from(option_env!("POLARIS_LOG_DIR").unwrap_or(".")).join("polaris.log"),
	);
	fs::create_dir_all(&log_file_path.parent().unwrap())?;
	info!("Log file location is {:#?}", log_file_path);
	init_logging(log_level, &log_file_path)?;

	// Create service context
	let mut context_builder = service::ContextBuilder::new();
	if let Some(port) = cli_options.port {
		context_builder = context_builder.port(port);
	}
	if let Some(path) = cli_options.config_file_path {
		info!("Config file location is {:#?}", path);
		context_builder = context_builder.config_file_path(path);
	}
	if let Some(path) = cli_options.database_file_path {
		context_builder = context_builder.database_file_path(path);
	}
	if let Some(path) = cli_options.web_dir_path {
		context_builder = context_builder.web_dir_path(path);
	}
	if let Some(path) = cli_options.swagger_dir_path {
		context_builder = context_builder.swagger_dir_path(path);
	}
	if let Some(path) = cli_options.cache_dir_path {
		context_builder = context_builder.cache_dir_path(path);
	}

	let context = context_builder.build()?;
	info!("Database file location is {:#?}", context.db.location());
	info!("Web client files location is {:#?}", context.web_dir_path);
	info!("Swagger files location is {:#?}", context.swagger_dir_path);
	info!(
		"Thumbnails files location is {:#?}",
		context.thumbnail_manager.get_directory()
	);

	// Begin collection scans
	context.index.begin_periodic_updates();

	// Start DDNS updates
	context.ddns_manager.begin_periodic_updates();

	// Start server
	info!("Starting up server");
	std::thread::spawn(move || {
		let _ = service::run(context);
	});

	// Send readiness notification
	notify_ready();

	// Run UI
	ui::run();

	info!("Shutting down server");
	Ok(())
}
