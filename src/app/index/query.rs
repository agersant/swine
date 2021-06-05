use anyhow::*;
use diesel;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types;
use regex::Regex;
use std::path::Path;

use super::*;
use crate::db::{directories, songs};

// A token is one of the field of song structure followed by ':' and a word or words within a
// single or double quotes.
// query should contain only one occurance of token.
// Ex. composer:"Some Composer" and lyricist:lyricist_name
fn parse_token(query: &String, token: &str) -> (Option<String>, String) {
	let mut substr = token.to_string();
	substr.push_str(":");
	let count = query.matches(&substr).count();

	if count == 0 || count > 1 {
		return (None, query.to_string());
	}

	// The query can be in the form
	// 1 'artist:artist_name generic_query'
	// 2 'generic_query artist:artist_name'
	// 3 'generic_query artist:artist_name generic_query'
	// In case of 2 and 3 we will have more than one string after split.
	let mut splits: Vec<&str> = query.split(&substr).collect();
	let mut query: String = "".to_string();
	if splits.len() > 1 {
		query.push_str(splits.remove(0).trim());
	}
	let re = Regex::new(r#""([^"]+)"|'([^']+)'|^([\w\-]+)"#).unwrap();
	let t = match re.find(&splits[0]) {
		Some(x) => x,
		None => {
			return (None, query);
		}
	};
	let artist = "%".to_string()
		+ splits[0][t.start()..t.end()]
			.replace('\'', "")
			.replace('"', "")
			.trim() + "%";
	let rest = splits[0][t.end()..].trim();

	if rest.len() > 0 {
		if query.len() == 0 {
			query = rest.to_string();
		} else {
			query.push_str(" ");
			query.push_str(rest);
		}
	}

	if artist.len() > 0 {
		return (Some(artist.to_string()), query.to_string());
	}

	(None, query)
}

#[derive(Default, Debug, PartialEq)]
pub struct QueryFields {
	pub title: Option<String>,
	pub artist: Option<String>,
	pub album_artist: Option<String>,
	pub album: Option<String>,
	pub lyricist: Option<String>,
	pub composer: Option<String>,
	pub genre: Option<String>,
	pub general_query: Option<String>,
}

pub fn parse_query(query: &str) -> QueryFields {
	// Replace multiple spaces and trim leading and trailing spaces.
	let re = Regex::new(r"\s+").unwrap();
	let query = re.replace_all(&query.to_ascii_lowercase(), " ").to_string();
	let query = query.trim().to_string();
	let (title, query) = parse_token(&query, "title");
	let (album_artist, query) = parse_token(&query, "album_artist");
	let (artist, query) = parse_token(&query, "artist");
	let (album, query) = parse_token(&query, "album");
	let (lyricist, query) = parse_token(&query, "lyricist");
	let (composer, query) = parse_token(&query, "composer");
	let (genre, query) = parse_token(&query, "genre");
	QueryFields {
		title,
		artist,
		album_artist,
		album,
		lyricist,
		composer,
		genre,
		general_query: Some(query),
	}
}

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
				let mut path_buf = real_path.clone();
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

	fn generic_search(&self, query: &str) -> Result<Vec<CollectionFile>> {
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
						.or(album_artist.like(&like_test))
						.or(composer.like(&like_test))
						.or(lyricist.like(&like_test))
						.or(genre.like(&like_test)),
				)
				.filter(parent.not_like(&like_test))
				.load(&connection)?;

			let virtual_songs = real_songs.into_iter().filter_map(|d| d.virtualize(&vfs));

			output.extend(virtual_songs.map(CollectionFile::Song));
		}

		Ok(output)
	}

	fn field_search(&self, fields: &QueryFields) -> Result<Vec<CollectionFile>> {
		let vfs = self.vfs_manager.get_vfs()?;
		let connection = self.db.connect()?;
		let mut output = Vec::new();

		// Find songs with matching title/album/artist and non-matching parent
		{
			use self::songs::dsl::*;
			let mut filter = songs.into_boxed();
			match fields.title.as_ref() {
				Some(title_name) => filter = filter.filter(title.like(title_name)),
				None => {}
			}
			match fields.artist.as_ref() {
				Some(artist_name) => filter = filter.filter(artist.like(artist_name)),
				None => {}
			}
			match fields.album_artist.as_ref() {
				Some(album_artist_name) => {
					filter = filter.filter(album_artist.like(album_artist_name))
				}
				None => {}
			}
			match fields.album.as_ref() {
				Some(album_name) => filter = filter.filter(album.like(album_name)),
				None => {}
			}
			match fields.lyricist.as_ref() {
				Some(lyricist_name) => filter = filter.filter(lyricist.like(lyricist_name)),
				None => {}
			}
			match fields.composer.as_ref() {
				Some(composer_name) => filter = filter.filter(composer.like(composer_name)),
				None => {}
			}
			match fields.genre.as_ref() {
				Some(genre_name) => filter = filter.filter(genre.like(genre_name)),
				None => {}
			}

			let real_songs: Vec<Song> = filter.load(&connection)?;
			let virtual_songs = real_songs.into_iter().filter_map(|d| d.virtualize(&vfs));

			output.extend(virtual_songs.map(CollectionFile::Song));
		}
		Ok(output)
	}

	pub fn search(&self, query: &str) -> Result<Vec<CollectionFile>> {
		let parsed_query = parse_query(query);
		let tmp = QueryFields {
			general_query: Some(parsed_query.general_query.as_ref().unwrap().to_string()),
			..Default::default()
		};
		if parsed_query == tmp {
			return self.generic_search(parsed_query.general_query.as_ref().unwrap());
		}
		self.field_search(&parsed_query)
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
