use std::path::PathBuf;

use crate::options::CLIOptions;

pub struct Paths {
	pub cache_dir_path: PathBuf,
	pub config_file_path: Option<PathBuf>,
	pub db_file_path: PathBuf,
	pub log_file_path: Option<PathBuf>,
	#[cfg(unix)]
	pub pid_file_path: PathBuf,
	pub swagger_dir_path: PathBuf,
	pub web_dir_path: PathBuf,
}

// TODO Make this the only implementation when we can expand %LOCALAPPDATA% correctly on Windows
// And fix the installer accordingly (`release_script.ps1`)
#[cfg(not(windows))]
impl Default for Paths {
	fn default() -> Self {
		Self {
			cache_dir_path: ["."].iter().collect(),
			config_file_path: None,
			db_file_path: [".", "db.sqlite"].iter().collect(),
			log_file_path: Some([".", "polaris.log"].iter().collect()),
			pid_file_path: [".", "polaris.pid"].iter().collect(),
			swagger_dir_path: [".", "docs", "swagger"].iter().collect(),
			web_dir_path: [".", "web"].iter().collect(),
		}
	}
}

#[cfg(windows)]
impl Default for Paths {
	fn default() -> Self {
		let local_app_data = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap();
		let install_directory: PathBuf =
			local_app_data.join(["Permafrost", "Polaris"].iter().collect::<PathBuf>());
		Self {
			cache_dir_path: install_directory.clone(),
			config_file_path: None,
			db_file_path: install_directory.join("db.sqlite"),
			log_file_path: Some(install_directory.join("polaris.log")),
			swagger_dir_path: install_directory.join("swagger"),
			web_dir_path: install_directory.join("web"),
		}
	}
}

impl Paths {
	fn from_build() -> Self {
		let defaults = Self::default();
		Self {
			db_file_path: option_env!("POLARIS_DB_DIR")
				.map(PathBuf::from)
				.map(|p| p.join("db.sqlite"))
				.unwrap_or(defaults.db_file_path),
			config_file_path: None,
			cache_dir_path: option_env!("POLARIS_CACHE_DIR")
				.map(PathBuf::from)
				.unwrap_or(defaults.cache_dir_path),
			log_file_path: option_env!("POLARIS_LOG_DIR")
				.map(PathBuf::from)
				.map(|p| p.join("polaris.log"))
				.or(defaults.log_file_path),
			#[cfg(unix)]
			pid_file_path: option_env!("POLARIS_PID_DIR")
				.map(PathBuf::from)
				.map(|p| p.join("polaris.pid"))
				.unwrap_or(defaults.pid_file_path),
			swagger_dir_path: option_env!("POLARIS_SWAGGER_DIR")
				.map(PathBuf::from)
				.unwrap_or(defaults.swagger_dir_path),
			web_dir_path: option_env!("POLARIS_WEB_DIR")
				.map(PathBuf::from)
				.unwrap_or(defaults.web_dir_path),
		}
	}

	pub fn new(cli_options: &CLIOptions) -> Self {
		let mut paths = Self::from_build();
		if let Some(path) = &cli_options.cache_dir_path {
			paths.cache_dir_path = path.clone();
		}
		if let Some(path) = &cli_options.config_file_path {
			paths.config_file_path = Some(path.clone());
		}
		if let Some(path) = &cli_options.database_file_path {
			paths.db_file_path = path.clone();
		}
		#[cfg(unix)]
		if let Some(path) = &cli_options.pid_file_path {
			paths.pid_file_path = path.clone();
		}
		if let Some(path) = &cli_options.swagger_dir_path {
			paths.swagger_dir_path = path.clone();
		}
		if let Some(path) = &cli_options.web_dir_path {
			paths.web_dir_path = path.clone();
		}

		let log_to_file = cli_options.log_file_path.is_some() || !cli_options.foreground;
		if log_to_file {
			paths.log_file_path = cli_options.log_file_path.clone().or(paths.log_file_path);
		} else {
			paths.log_file_path = None;
		};

		paths
	}
}
