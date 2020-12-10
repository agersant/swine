use anyhow::*;
use rustfm_scrobble::{Scrobble, Scrobbler};
use serde::Deserialize;
use std::path::Path;

use crate::app::index::Index;
use crate::db::DB;
use crate::user;

const LASTFM_API_KEY: &str = "02b96c939a2b451c31dfd67add1f696e";
const LASTFM_API_SECRET: &str = "0f25a80ceef4b470b5cb97d99d4b3420";

#[derive(Debug, Deserialize)]
struct AuthResponseSessionName {
	#[serde(rename = "$value")]
	pub body: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponseSessionKey {
	#[serde(rename = "$value")]
	pub body: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponseSessionSubscriber {
	#[serde(rename = "$value")]
	pub body: i32,
}

#[derive(Debug, Deserialize)]
struct AuthResponseSession {
	pub name: AuthResponseSessionName,
	pub key: AuthResponseSessionKey,
	pub subscriber: AuthResponseSessionSubscriber,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
	pub status: String,
	pub session: AuthResponseSession,
}

pub struct Manager {
	db: DB,
	index: Index,
}

impl Manager {
	pub fn new(db: DB, index: Index) -> Self {
		Self { db, index }
	}

	pub fn link(&self, username: &str, token: &str) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY.into(), LASTFM_API_SECRET.into());
		let auth_response = scrobbler.authenticate_with_token(token)?;

		user::lastfm_link(&self.db, username, &auth_response.name, &auth_response.key)
	}

	pub fn unlink(&self, username: &str) -> Result<()> {
		user::lastfm_unlink(&self.db, username)
	}

	pub fn scrobble(&self, username: &str, track: &Path) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY.into(), LASTFM_API_SECRET.into());
		let scrobble = self.scrobble_from_path(track)?;
		let auth_token = user::get_lastfm_session_key(&self.db, username)?;
		scrobbler.authenticate_with_session_key(&auth_token);
		scrobbler.scrobble(&scrobble)?;
		Ok(())
	}

	pub fn now_playing(&self, username: &str, track: &Path) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY.into(), LASTFM_API_SECRET.into());
		let scrobble = self.scrobble_from_path(track)?;
		let auth_token = user::get_lastfm_session_key(&self.db, username)?;
		scrobbler.authenticate_with_session_key(&auth_token);
		scrobbler.now_playing(&scrobble)?;
		Ok(())
	}

	fn scrobble_from_path(&self, track: &Path) -> Result<Scrobble> {
		let song = self.index.get_song(track)?;
		Ok(Scrobble::new(
			song.artist.as_deref().unwrap_or(""),
			song.title.as_deref().unwrap_or(""),
			song.album.as_deref().unwrap_or(""),
		))
	}
}
