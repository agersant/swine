use anyhow::*;
use rustfm_scrobble::{Scrobble, Scrobbler};
use serde::Deserialize;
use std::path::Path;
use user::AuthToken;

use crate::app::{index::Index, user};

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

#[derive(Clone)]
pub struct Manager {
	index: Index,
	user_manager: user::Manager,
}

impl Manager {
	pub fn new(index: Index, user_manager: user::Manager) -> Self {
		Self {
			index,
			user_manager,
		}
	}

	pub fn generate_link_token(&self, username: &str) -> Result<AuthToken> {
		self.user_manager
			.generate_lastfm_link_token(username)
			.map_err(|e| e.into())
	}

	pub fn link(&self, username: &str, lastfm_token: &str) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY, LASTFM_API_SECRET);
		let auth_response = scrobbler.authenticate_with_token(lastfm_token)?;

		self.user_manager
			.lastfm_link(username, &auth_response.name, &auth_response.key)
			.map_err(|e| e.into())
	}

	pub fn unlink(&self, username: &str) -> Result<()> {
		self.user_manager.lastfm_unlink(username)
	}

	pub fn scrobble(&self, username: &str, track: &Path) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY, LASTFM_API_SECRET);
		let scrobble = self.scrobble_from_path(track)?;
		let auth_token = self.user_manager.get_lastfm_session_key(username)?;
		scrobbler.authenticate_with_session_key(&auth_token);
		scrobbler.scrobble(&scrobble)?;
		Ok(())
	}

	pub fn now_playing(&self, username: &str, track: &Path) -> Result<()> {
		let mut scrobbler = Scrobbler::new(LASTFM_API_KEY, LASTFM_API_SECRET);
		let scrobble = self.scrobble_from_path(track)?;
		let auth_token = self.user_manager.get_lastfm_session_key(username)?;
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
