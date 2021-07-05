use anyhow::*;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types;
use std::path::Path;

use super::*;
use crate::db::{directories, songs};

#[derive(thiserror::Error, Debug)]
pub enum QueryError {
	#[error("VFS path not found")]
	VFSPathNotFound,
	#[error("Unspecified")]
	Unspecified,
}

impl From<anyhow::Error> for QueryError {
	fn from(_: anyhow::Error) -> Self {
		QueryError::Unspecified
	}
}

no_arg_sql_function!(
	random,
	sql_types::Integer,
	"Represents the SQL RANDOM() function"
);

impl Index {
	pub fn browse<P>(&self, virtual_path: P) -> Result<Vec<CollectionFile>, QueryError>
	where
		P: AsRef<Path>,
	{
		let mut output = Vec::new();
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;

		if virtual_path.as_ref().components().count() == 0 {
			// Browse top-level
			let real_directories: Vec<Directory> = directories::table
				.filter(directories::parent.is_null())
				.load(&connection)
				.map_err(anyhow::Error::new)?;
			let virtual_directories = real_directories
				.into_iter()
				.filter_map(|d| d.virtualize(&vfs));
			output.extend(virtual_directories.map(CollectionFile::Directory));
		} else {
			// Browse sub-directory
			let real_path = vfs
				.virtual_to_real(virtual_path)
				.map_err(|_| QueryError::VFSPathNotFound)?;
			let real_path_string = real_path.as_path().to_string_lossy().into_owned();

			let real_directories: Vec<Directory> = directories::table
				.filter(directories::parent.eq(&real_path_string))
				.order(sql::<sql_types::Bool>("path COLLATE NOCASE ASC"))
				.load(&connection)
				.map_err(anyhow::Error::new)?;
			let virtual_directories = real_directories
				.into_iter()
				.filter_map(|d| d.virtualize(&vfs));
			output.extend(virtual_directories.map(CollectionFile::Directory));

			let real_songs: Vec<Song> = songs::table
				.filter(songs::parent.eq(&real_path_string))
				.order(sql::<sql_types::Bool>("path COLLATE NOCASE ASC"))
				.load(&connection)
				.map_err(anyhow::Error::new)?;
			let virtual_songs = real_songs.into_iter().filter_map(|s| s.virtualize(&vfs));
			output.extend(virtual_songs.map(CollectionFile::Song));
		}

		Ok(output)
	}

	pub fn flatten<P>(&self, virtual_path: P) -> Result<Vec<Song>, QueryError>
	where
		P: AsRef<Path>,
	{
		use self::songs::dsl::*;
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;

		let real_songs: Vec<Song> = if virtual_path.as_ref().parent() != None {
			let real_path = vfs
				.virtual_to_real(virtual_path)
				.map_err(|_| QueryError::VFSPathNotFound)?;
			let song_path_filter = {
				let mut path_buf = real_path;
				path_buf.push("%");
				path_buf.as_path().to_string_lossy().into_owned()
			};
			songs
				.filter(path.like(&song_path_filter))
				.order(path)
				.load(&connection)
				.map_err(anyhow::Error::new)?
		} else {
			songs
				.order(path)
				.load(&connection)
				.map_err(anyhow::Error::new)?
		};

		let virtual_songs = real_songs.into_iter().filter_map(|s| s.virtualize(&vfs));
		Ok(virtual_songs.collect::<Vec<_>>())
	}

	pub fn get_random_albums(&self, count: i64) -> Result<Vec<Directory>> {
		use self::directories::dsl::*;
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;
		let real_directories: Vec<Directory> = directories
			.filter(album.is_not_null())
			.limit(count)
			.order(random)
			.load(&connection)?;
		let virtual_directories = real_directories
			.into_iter()
			.filter_map(|d| d.virtualize(&vfs));
		Ok(virtual_directories.collect::<Vec<_>>())
	}

	pub fn get_recent_albums(&self, count: i64) -> Result<Vec<Directory>> {
		use self::directories::dsl::*;
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;
		let real_directories: Vec<Directory> = directories
			.filter(album.is_not_null())
			.order(date_added.desc())
			.limit(count)
			.load(&connection)?;
		let virtual_directories = real_directories
			.into_iter()
			.filter_map(|d| d.virtualize(&vfs));
		Ok(virtual_directories.collect::<Vec<_>>())
	}

	pub fn search(&self, query: &str) -> Result<Vec<CollectionFile>> {
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;
		let like_test = format!("%{}%", query);
		let mut output = Vec::new();

		// Find dirs with matching path and parent not matching
		{
			use self::directories::dsl::*;
			let real_directories: Vec<Directory> = directories
				.filter(path.like(&like_test))
				.filter(parent.not_like(&like_test))
				.load(&connection)?;

			let virtual_directories = real_directories
				.into_iter()
				.filter_map(|d| d.virtualize(&vfs));

			output.extend(virtual_directories.map(CollectionFile::Directory));
		}

		// Find songs with matching title/album/artist and non-matching parent
		{
			use self::songs::dsl::*;
			let real_songs: Vec<Song> = songs
				.filter(
					path.like(&like_test)
						.or(title.like(&like_test))
						.or(album.like(&like_test))
						.or(artist.like(&like_test))
						.or(album_artist.like(&like_test)),
				)
				.filter(parent.not_like(&like_test))
				.load(&connection)?;

			let virtual_songs = real_songs.into_iter().filter_map(|d| d.virtualize(&vfs));

			output.extend(virtual_songs.map(CollectionFile::Song));
		}

		Ok(output)
	}

	pub fn get_song(&self, virtual_path: &Path) -> Result<Song> {
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;

		let real_path = vfs.virtual_to_real(virtual_path)?;
		let real_path_string = real_path.as_path().to_string_lossy();

		use self::songs::dsl::*;
		let real_song: Song = songs
			.filter(path.eq(real_path_string))
			.get_result(&connection)?;

		match real_song.virtualize(&vfs) {
			Some(s) => Ok(s),
			_ => bail!("Missing VFS mapping"),
		}
	}
}
