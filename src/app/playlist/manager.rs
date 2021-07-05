use anyhow::Result;
use core::clone::Clone;
use diesel::prelude::*;
use diesel::sql_types;
use diesel::BelongingToDsl;
use std::path::Path;

use super::*;
use crate::app::index::Song;
use crate::app::vfs;
use crate::db::{playlist_songs, playlists, users, DB};

#[derive(Clone)]
pub struct Manager {
	db: DB,
	vfs_manager: vfs::Manager,
}

impl Manager {
	pub fn new(db: DB, vfs_manager: vfs::Manager) -> Self {
		Self { db, vfs_manager }
	}

	pub fn list_playlists(&self, owner: &str) -> Result<Vec<String>, Error> {
		let connection = self.db.connect()?;

		let user: User = {
			use self::users::dsl::*;
			users
				.filter(name.eq(owner))
				.select((id,))
				.first(&connection)
				.optional()
				.map_err(anyhow::Error::new)?
				.ok_or(Error::UserNotFound)?
		};

		{
			use self::playlists::dsl::*;
			let found_playlists: Vec<String> = Playlist::belonging_to(&user)
				.select(name)
				.load(&connection)
				.map_err(anyhow::Error::new)?;
			Ok(found_playlists)
		}
	}

	pub fn save_playlist(
		&self,
		playlist_name: &str,
		owner: &str,
		content: &[String],
	) -> Result<(), Error> {
		let new_playlist: NewPlaylist;
		let playlist: Playlist;
		let vfs = self.vfs_manager.get_vfs()?;

		{
			let connection = self.db.connect()?;

			// Find owner
			let user: User = {
				use self::users::dsl::*;
				users
					.filter(name.eq(owner))
					.select((id,))
					.first(&connection)
					.optional()
					.map_err(anyhow::Error::new)?
					.ok_or(Error::UserNotFound)?
			};

			// Create playlist
			new_playlist = NewPlaylist {
				name: playlist_name.into(),
				owner: user.id,
			};

			diesel::insert_into(playlists::table)
				.values(&new_playlist)
				.execute(&connection)
				.map_err(anyhow::Error::new)?;

			playlist = {
				use self::playlists::dsl::*;
				playlists
					.select((id, owner))
					.filter(name.eq(playlist_name).and(owner.eq(user.id)))
					.get_result(&connection)
					.map_err(anyhow::Error::new)?
			}
		}

		let mut new_songs: Vec<NewPlaylistSong> = Vec::new();
		new_songs.reserve(content.len());

		for (i, path) in content.iter().enumerate() {
			let virtual_path = Path::new(&path);
			if let Some(real_path) = vfs
				.virtual_to_real(virtual_path)
				.ok()
				.and_then(|p| p.to_str().map(|s| s.to_owned()))
			{
				new_songs.push(NewPlaylistSong {
					playlist: playlist.id,
					path: real_path,
					ordering: i as i32,
				});
			}
		}

		{
			let connection = self.db.connect()?;
			connection
				.transaction::<_, diesel::result::Error, _>(|| {
					// Delete old content (if any)
					let old_songs = PlaylistSong::belonging_to(&playlist);
					diesel::delete(old_songs).execute(&connection)?;

					// Insert content
					diesel::insert_into(playlist_songs::table)
						.values(&new_songs)
						.execute(&*connection)?; // TODO https://github.com/diesel-rs/diesel/issues/1822
					Ok(())
				})
				.map_err(anyhow::Error::new)?;
		}

		Ok(())
	}

	pub fn read_playlist(&self, playlist_name: &str, owner: &str) -> Result<Vec<Song>, Error> {
		let vfs = self.vfs_manager.get_vfs()?;
		let songs: Vec<Song>;

		{
			let connection = self.db.connect()?;

			// Find owner
			let user: User = {
				use self::users::dsl::*;
				users
					.filter(name.eq(owner))
					.select((id,))
					.first(&connection)
					.optional()
					.map_err(anyhow::Error::new)?
					.ok_or(Error::UserNotFound)?
			};

			// Find playlist
			let playlist: Playlist = {
				use self::playlists::dsl::*;
				playlists
					.select((id, owner))
					.filter(name.eq(playlist_name).and(owner.eq(user.id)))
					.get_result(&connection)
					.optional()
					.map_err(anyhow::Error::new)?
					.ok_or(Error::PlaylistNotFound)?
			};

			// Select songs. Not using Diesel because we need to LEFT JOIN using a custom column
			let query = diesel::sql_query(
				r#"
			SELECT s.id, s.path, s.parent, s.track_number, s.disc_number, s.title, s.artist, s.album_artist, s.year, s.album, s.artwork, s.duration, s.lyricist, s.composer, s.genre, s.label
			FROM playlist_songs ps
			LEFT JOIN songs s ON ps.path = s.path
			WHERE ps.playlist = ?
			ORDER BY ps.ordering
		"#,
			);
			let query = query.bind::<sql_types::Integer, _>(playlist.id);
			songs = query.get_results(&connection).map_err(anyhow::Error::new)?;
		}

		// Map real path to virtual paths
		let virtual_songs = songs
			.into_iter()
			.filter_map(|s| s.virtualize(&vfs))
			.collect();

		Ok(virtual_songs)
	}

	pub fn delete_playlist(&self, playlist_name: &str, owner: &str) -> Result<(), Error> {
		let connection = self.db.connect()?;

		let user: User = {
			use self::users::dsl::*;
			users
				.filter(name.eq(owner))
				.select((id,))
				.first(&connection)
				.optional()
				.map_err(anyhow::Error::new)?
				.ok_or(Error::UserNotFound)?
		};

		{
			use self::playlists::dsl::*;
			let q = Playlist::belonging_to(&user).filter(name.eq(playlist_name));
			match diesel::delete(q)
				.execute(&connection)
				.map_err(anyhow::Error::new)?
			{
				0 => Err(Error::PlaylistNotFound),
				_ => Ok(()),
			}
		}
	}
}

#[derive(Identifiable, Queryable, Associations)]
#[belongs_to(User, foreign_key = "owner")]
struct Playlist {
	id: i32,
	owner: i32,
}

#[derive(Identifiable, Queryable, Associations)]
#[belongs_to(Playlist, foreign_key = "playlist")]
struct PlaylistSong {
	id: i32,
	playlist: i32,
}

#[derive(Insertable)]
#[table_name = "playlists"]
struct NewPlaylist {
	name: String,
	owner: i32,
}

#[derive(Insertable)]
#[table_name = "playlist_songs"]
struct NewPlaylistSong {
	playlist: i32,
	path: String,
	ordering: i32,
}

#[derive(Identifiable, Queryable)]
struct User {
	id: i32,
}
