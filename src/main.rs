#![cfg_attr(all(windows, feature = "ui"), windows_subsystem = "windows")]
#![recursion_limit = "256"]

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use anyhow::*;
use log::info;
use simplelog::{CombinedLogger, LevelFilter, SharedLogger, TermLogger, TerminalMode, WriteLogger};
use std::fs;
use std::path::Path;

mod app;
mod db;
mod options;
mod paths;
mod service;
#[cfg(test)]
mod test;
mod ui;
mod utils;

#[cfg(unix)]
fn daemonize<T: AsRef<Path>>(foreground: bool, pid_file_path: T) -> Result<()> {
	if foreground {
		return Ok(());
	}
	if let Some(parent) = pid_file_path.as_ref().parent() {
		fs::create_dir_all(parent)?;
	}
	let daemonize = daemonize::Daemonize::new()
		.pid_file(pid_file_path.as_ref())
		.working_directory(".");
	daemonize.start()?;
	Ok(())
}

#[cfg(unix)]
fn notify_ready() -> Result<()> {
	if let Ok(true) = sd_notify::booted() {
		sd_notify::notify(true, &[sd_notify::NotifyState::Ready])?;
	}
	Ok(())
}

fn init_logging<T: AsRef<Path>>(log_level: LevelFilter, log_file_path: &Option<T>) -> Result<()> {
	let log_config = simplelog::ConfigBuilder::new()
		.set_location_level(LevelFilter::Error)
		.build();

	let mut loggers: Vec<Box<dyn SharedLogger>> = vec![TermLogger::new(
		log_level,
		log_config.clone(),
		TerminalMode::Mixed,
	)];

	if let Some(path) = log_file_path {
		if let Some(parent) = path.as_ref().parent() {
			fs::create_dir_all(parent)?;
		}
		loggers.push(WriteLogger::new(
			log_level,
			log_config,
			fs::File::create(path)?,
		));
	}

	CombinedLogger::init(loggers)?;

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

	let paths = paths::Paths::new(&cli_options);

	// Logging
	let log_level = cli_options.log_level.unwrap_or(LevelFilter::Info);
	init_logging(log_level, &paths.log_file_path)?;

	// Fork
	#[cfg(unix)]
	daemonize(cli_options.foreground, &paths.pid_file_path)?;

	info!("Cache files location is {:#?}", paths.cache_dir_path);
	info!("Config files location is {:#?}", paths.config_file_path);
	info!("Database file location is {:#?}", paths.db_file_path);
	info!("Log file location is {:#?}", paths.log_file_path);
	#[cfg(unix)]
	if !cli_options.foreground {
		info!("Pid file location is {:#?}", paths.pid_file_path);
	}
	info!("Swagger files location is {:#?}", paths.swagger_dir_path);
	info!("Web client files location is {:#?}", paths.web_dir_path);

	// Create and run app
	let app = app::App::new(cli_options.port.unwrap_or(5050), paths)?;
	app.index.begin_periodic_updates();
	app.ddns_manager.begin_periodic_updates();

	// Start server
	info!("Starting up server");
	std::thread::spawn(move || {
		let _ = service::run(app);
	});

	// Send readiness notification
	#[cfg(unix)]
	notify_ready()?;

	// Run UI
	ui::run();

	info!("Shutting down server");
	Ok(())
}
