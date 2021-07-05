use diesel::prelude::*;
use regex::Regex;
use std::convert::TryInto;
use std::time::Duration;

use super::*;
use crate::db::{misc_settings, DB};

#[derive(Clone)]
pub struct Manager {
	pub db: DB,
}

impl Manager {
	pub fn new(db: DB) -> Self {
		Self { db }
	}

	pub fn get_auth_secret(&self) -> Result<AuthSecret, Error> {
		use self::misc_settings::dsl::*;
		let connection = self.db.connect()?;
		let secret: Vec<u8> = misc_settings
			.select(auth_secret)
			.get_result(&connection)
			.map_err(|e| match e {
				diesel::result::Error::NotFound => Error::AuthSecretNotFound,
				_ => Error::Unspecified,
			})?;
		secret
			.try_into()
			.map_err(|_| Error::InvalidAuthSecret)
			.map(|key| AuthSecret { key })
	}

	pub fn get_index_sleep_duration(&self) -> Result<Duration, Error> {
		use self::misc_settings::dsl::*;
		let connection = self.db.connect()?;
		misc_settings
			.select(index_sleep_duration_seconds)
			.get_result(&connection)
			.map_err(|e| match e {
				diesel::result::Error::NotFound => Error::IndexSleepDurationNotFound,
				_ => Error::Unspecified,
			})
			.map(|s: i32| Duration::from_secs(s as u64))
	}

	pub fn get_index_album_art_pattern(&self) -> Result<Regex, Error> {
		use self::misc_settings::dsl::*;
		let connection = self.db.connect()?;
		misc_settings
			.select(index_album_art_pattern)
			.get_result(&connection)
			.map_err(|e| match e {
				diesel::result::Error::NotFound => Error::IndexAlbumArtPatternNotFound,
				_ => Error::Unspecified,
			})
			.and_then(|s: String| {
				Regex::new(&format!("(?i){}", &s)).map_err(|_| Error::IndexAlbumArtPatternInvalid)
			})
	}

	pub fn read(&self) -> Result<Settings, Error> {
		let connection = self.db.connect()?;

		let misc: MiscSettings = misc_settings::table
			.get_result(&connection)
			.map_err(|_| Error::Unspecified)?;

		Ok(Settings {
			auth_secret: misc.auth_secret,
			album_art_pattern: misc.index_album_art_pattern,
			reindex_every_n_seconds: misc.index_sleep_duration_seconds,
		})
	}

	pub fn amend(&self, new_settings: &NewSettings) -> Result<(), Error> {
		let connection = self.db.connect()?;

		if let Some(sleep_duration) = new_settings.reindex_every_n_seconds {
			diesel::update(misc_settings::table)
				.set(misc_settings::index_sleep_duration_seconds.eq(sleep_duration as i32))
				.execute(&connection)
				.map_err(|_| Error::Unspecified)?;
		}

		if let Some(ref album_art_pattern) = new_settings.album_art_pattern {
			diesel::update(misc_settings::table)
				.set(misc_settings::index_album_art_pattern.eq(album_art_pattern))
				.execute(&connection)
				.map_err(|_| Error::Unspecified)?;
		}

		Ok(())
	}
}
