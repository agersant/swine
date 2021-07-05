use anyhow::*;
use diesel::prelude::*;
use rayon::prelude::*;
use std::path::Path;

use crate::app::vfs;
use crate::db::{directories, songs, DB};

const INDEX_BUILDING_CLEAN_BUFFER_SIZE: usize = 500; // Deletions in each transaction

pub struct Cleaner {
	db: DB,
	vfs_manager: vfs::Manager,
}

impl Cleaner {
	pub fn new(db: DB, vfs_manager: vfs::Manager) -> Self {
		Self { db, vfs_manager }
	}

	pub fn clean(&self) -> Result<()> {
		let vfs = self.vfs_manager.get_vfs()?;

		let all_directories: Vec<String> = {
			let connection = self.db.connect()?;
			directories::table
				.select(directories::path)
				.load(&connection)?
		};

		let all_songs: Vec<String> = {
			let connection = self.db.connect()?;
			songs::table.select(songs::path).load(&connection)?
		};

		let list_missing_directories = || {
			all_directories
				.par_iter()
				.filter(|ref directory_path| {
					let path = Path::new(&directory_path);
					!path.exists() || vfs.real_to_virtual(path).is_err()
				})
				.collect::<Vec<_>>()
		};

		let list_missing_songs = || {
			all_songs
				.par_iter()
				.filter(|ref song_path| {
					let path = Path::new(&song_path);
					!path.exists() || vfs.real_to_virtual(path).is_err()
				})
				.collect::<Vec<_>>()
		};

		let thread_pool = rayon::ThreadPoolBuilder::new().build()?;
		let (missing_directories, missing_songs) =
			thread_pool.join(list_missing_directories, list_missing_songs);

		{
			let connection = self.db.connect()?;
			for chunk in missing_directories[..].chunks(INDEX_BUILDING_CLEAN_BUFFER_SIZE) {
				diesel::delete(directories::table.filter(directories::path.eq_any(chunk)))
					.execute(&connection)?;
			}
			for chunk in missing_songs[..].chunks(INDEX_BUILDING_CLEAN_BUFFER_SIZE) {
				diesel::delete(songs::table.filter(songs::path.eq_any(chunk)))
					.execute(&connection)?;
			}
		}

		Ok(())
	}
}
