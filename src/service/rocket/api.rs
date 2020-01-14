use anyhow::*;
use rocket::http::{Cookie, Cookies, RawStr, Status};
use rocket::request::{self, FromParam, FromRequest, Request};
use rocket::response::content::Html;
use rocket::{delete, get, post, put, routes, Outcome, State};
use rocket_contrib::json::Json;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::ops::Deref;
use std::path::PathBuf;
use std::str;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use time::Duration;

use super::serve;
use crate::config::{self, Config, Preferences};
use crate::db::DB;
use crate::index;
use crate::lastfm;
use crate::playlist;
use crate::thumbnails;
use crate::user;
use crate::utils;
use crate::vfs::VFSSource;

const CURRENT_MAJOR_VERSION: i32 = 4;
const CURRENT_MINOR_VERSION: i32 = 0;
const COOKIE_SESSION: &str = "session";
const COOKIE_USERNAME: &str = "username";
const COOKIE_ADMIN: &str = "admin";

pub fn get_routes() -> Vec<rocket::Route> {
	routes![
		version,
		initial_setup,
		get_settings,
		put_settings,
		get_preferences,
		put_preferences,
		trigger_index,
		auth,
		browse_root,
		browse,
		flatten_root,
		flatten,
		random,
		recent,
		search_root,
		search,
		serve,
		list_playlists,
		save_playlist,
		read_playlist,
		delete_playlist,
		lastfm_link,
		lastfm_unlink,
		lastfm_now_playing,
		lastfm_scrobble,
	]
}

#[derive(Error, Debug)]
enum APIError {
	#[error("Incorrect Credentials")]
	IncorrectCredentials,
	#[error("Unspecified")]
	Unspecified,
}

impl<'r> rocket::response::Responder<'r> for APIError {
	fn respond_to(self, _: &rocket::request::Request<'_>) -> rocket::response::Result<'r> {
		let status = match self {
			APIError::IncorrectCredentials => rocket::http::Status::Unauthorized,
			_ => rocket::http::Status::InternalServerError,
		};
		rocket::response::Response::build().status(status).ok()
	}
}

impl From<anyhow::Error> for APIError {
	fn from(_: anyhow::Error) -> Self {
		APIError::Unspecified
	}
}

struct Auth {
	username: String,
}

fn add_session_cookies(cookies: &mut Cookies, username: &str, is_admin: bool) -> () {
	let duration = Duration::days(1);

	let session_cookie = Cookie::build(COOKIE_SESSION, username.to_owned())
		.same_site(rocket::http::SameSite::Lax)
		.http_only(true)
		.max_age(duration)
		.finish();

	let username_cookie = Cookie::build(COOKIE_USERNAME, username.to_owned())
		.same_site(rocket::http::SameSite::Lax)
		.http_only(false)
		.max_age(duration)
		.path("/")
		.finish();

	let is_admin_cookie = Cookie::build(COOKIE_ADMIN, format!("{}", is_admin))
		.same_site(rocket::http::SameSite::Lax)
		.http_only(false)
		.max_age(duration)
		.path("/")
		.finish();

	cookies.add_private(session_cookie);
	cookies.add(username_cookie);
	cookies.add(is_admin_cookie);
}

impl<'a, 'r> FromRequest<'a, 'r> for Auth {
	type Error = ();

	fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, ()> {
		let mut cookies = request.guard::<Cookies<'_>>().unwrap();
		let db = match request.guard::<State<'_, Arc<DB>>>() {
			Outcome::Success(d) => d,
			_ => return Outcome::Failure((Status::InternalServerError, ())),
		};

		if let Some(u) = cookies.get_private(COOKIE_SESSION) {
			let exists = match user::exists(db.deref().deref(), u.value()) {
				Ok(e) => e,
				Err(_) => return Outcome::Failure((Status::InternalServerError, ())),
			};
			if !exists {
				return Outcome::Failure((Status::Unauthorized, ()));
			}
			return Outcome::Success(Auth {
				username: u.value().to_string(),
			});
		}

		if let Some(auth_header_string) = request.headers().get_one("Authorization") {
			use rocket::http::hyper::header::*;
			if let Ok(Basic {
				username,
				password: Some(password),
			}) = Basic::from_str(auth_header_string.trim_start_matches("Basic "))
			{
				if user::auth(db.deref().deref(), &username, &password).unwrap_or(false) {
					let is_admin = match user::is_admin(db.deref().deref(), &username) {
						Ok(a) => a,
						Err(_) => return Outcome::Failure((Status::InternalServerError, ())),
					};
					add_session_cookies(&mut cookies, &username, is_admin);
					return Outcome::Success(Auth {
						username: username.to_string(),
					});
				}
			}
		}

		Outcome::Failure((Status::Unauthorized, ()))
	}
}

struct AdminRights {}
impl<'a, 'r> FromRequest<'a, 'r> for AdminRights {
	type Error = ();

	fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, ()> {
		let db = request.guard::<State<'_, Arc<DB>>>()?;

		match user::count::<DB>(&db) {
			Err(_) => return Outcome::Failure((Status::InternalServerError, ())),
			Ok(0) => return Outcome::Success(AdminRights {}),
			_ => (),
		};

		let auth = request.guard::<Auth>()?;
		match user::is_admin::<DB>(&db, &auth.username) {
			Err(_) => Outcome::Failure((Status::InternalServerError, ())),
			Ok(true) => Outcome::Success(AdminRights {}),
			Ok(false) => Outcome::Failure((Status::Forbidden, ())),
		}
	}
}

struct VFSPathBuf {
	path_buf: PathBuf,
}

impl<'r> FromParam<'r> for VFSPathBuf {
	type Error = &'r RawStr;

	fn from_param(param: &'r RawStr) -> Result<Self, Self::Error> {
		let decoded_path = param.percent_decode_lossy();
		Ok(VFSPathBuf {
			path_buf: PathBuf::from(decoded_path.into_owned()),
		})
	}
}

impl From<VFSPathBuf> for PathBuf {
	fn from(vfs_path_buf: VFSPathBuf) -> Self {
		vfs_path_buf.path_buf.clone()
	}
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
pub struct Version {
	pub major: i32,
	pub minor: i32,
}

#[get("/version")]
fn version() -> Json<Version> {
	let current_version = Version {
		major: CURRENT_MAJOR_VERSION,
		minor: CURRENT_MINOR_VERSION,
	};
	Json(current_version)
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
pub struct InitialSetup {
	pub has_any_users: bool,
}

#[get("/initial_setup")]
fn initial_setup(db: State<'_, Arc<DB>>) -> Result<Json<InitialSetup>> {
	let initial_setup = InitialSetup {
		has_any_users: user::count::<DB>(&db)? > 0,
	};
	Ok(Json(initial_setup))
}

#[get("/settings")]
fn get_settings(db: State<'_, Arc<DB>>, _admin_rights: AdminRights) -> Result<Json<Config>> {
	let config = config::read::<DB>(&db)?;
	Ok(Json(config))
}

#[put("/settings", data = "<config>")]
fn put_settings(
	db: State<'_, Arc<DB>>,
	_admin_rights: AdminRights,
	config: Json<Config>,
) -> Result<()> {
	config::amend::<DB>(&db, &config)?;
	Ok(())
}

#[get("/preferences")]
fn get_preferences(db: State<'_, Arc<DB>>, auth: Auth) -> Result<Json<Preferences>> {
	let preferences = config::read_preferences::<DB>(&db, &auth.username)?;
	Ok(Json(preferences))
}

#[put("/preferences", data = "<preferences>")]
fn put_preferences(
	db: State<'_, Arc<DB>>,
	auth: Auth,
	preferences: Json<Preferences>,
) -> Result<()> {
	config::write_preferences::<DB>(&db, &auth.username, &preferences)?;
	Ok(())
}

#[post("/trigger_index")]
fn trigger_index(
	command_sender: State<'_, Arc<index::CommandSender>>,
	_admin_rights: AdminRights,
) -> Result<()> {
	command_sender.trigger_reindex()?;
	Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct AuthCredentials {
	pub username: String,
	pub password: String,
}

#[derive(Serialize)]
struct AuthOutput {
	admin: bool,
}

#[post("/auth", data = "<credentials>")]
fn auth(
	db: State<'_, Arc<DB>>,
	credentials: Json<AuthCredentials>,
	mut cookies: Cookies<'_>,
) -> std::result::Result<(), APIError> {
	if !user::auth::<DB>(&db, &credentials.username, &credentials.password)? {
		return Err(APIError::IncorrectCredentials);
	}
	let is_admin = user::is_admin::<DB>(&db, &credentials.username)?;
	add_session_cookies(&mut cookies, &credentials.username, is_admin);
	Ok(())
}

#[get("/browse")]
fn browse_root(db: State<'_, Arc<DB>>, _auth: Auth) -> Result<Json<Vec<index::CollectionFile>>> {
	let result = index::browse(db.deref().deref(), &PathBuf::new())?;
	Ok(Json(result))
}

#[get("/browse/<path>")]
fn browse(
	db: State<'_, Arc<DB>>,
	_auth: Auth,
	path: VFSPathBuf,
) -> Result<Json<Vec<index::CollectionFile>>> {
	let result = index::browse(db.deref().deref(), &path.into() as &PathBuf)?;
	Ok(Json(result))
}

#[get("/flatten")]
fn flatten_root(db: State<'_, Arc<DB>>, _auth: Auth) -> Result<Json<Vec<index::Song>>> {
	let result = index::flatten(db.deref().deref(), &PathBuf::new())?;
	Ok(Json(result))
}

#[get("/flatten/<path>")]
fn flatten(
	db: State<'_, Arc<DB>>,
	_auth: Auth,
	path: VFSPathBuf,
) -> Result<Json<Vec<index::Song>>> {
	let result = index::flatten(db.deref().deref(), &path.into() as &PathBuf)?;
	Ok(Json(result))
}

#[get("/random")]
fn random(db: State<'_, Arc<DB>>, _auth: Auth) -> Result<Json<Vec<index::Directory>>> {
	let result = index::get_random_albums(db.deref().deref(), 20)?;
	Ok(Json(result))
}

#[get("/recent")]
fn recent(db: State<'_, Arc<DB>>, _auth: Auth) -> Result<Json<Vec<index::Directory>>> {
	let result = index::get_recent_albums(db.deref().deref(), 20)?;
	Ok(Json(result))
}

#[get("/search")]
fn search_root(db: State<'_, Arc<DB>>, _auth: Auth) -> Result<Json<Vec<index::CollectionFile>>> {
	let result = index::search(db.deref().deref(), "")?;
	Ok(Json(result))
}

#[get("/search/<query>")]
fn search(
	db: State<'_, Arc<DB>>,
	_auth: Auth,
	query: String,
) -> Result<Json<Vec<index::CollectionFile>>> {
	let result = index::search(db.deref().deref(), &query)?;
	Ok(Json(result))
}

#[get("/serve/<path>")]
fn serve(
	db: State<'_, Arc<DB>>,
	_auth: Auth,
	path: VFSPathBuf,
) -> Result<serve::RangeResponder<File>> {
	let db: &DB = db.deref().deref();
	let vfs = db.get_vfs()?;
	let real_path = vfs.virtual_to_real(&path.into() as &PathBuf)?;

	let serve_path = if utils::is_image(&real_path) {
		thumbnails::get_thumbnail(&real_path, 400)?
	} else {
		real_path
	};

	let file = File::open(serve_path)?;
	Ok(serve::RangeResponder::new(file))
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ListPlaylistsEntry {
	pub name: String,
}

#[get("/playlists")]
fn list_playlists(db: State<'_, Arc<DB>>, auth: Auth) -> Result<Json<Vec<ListPlaylistsEntry>>> {
	let playlist_names = playlist::list_playlists(&auth.username, db.deref().deref())?;
	let playlists: Vec<ListPlaylistsEntry> = playlist_names
		.into_iter()
		.map(|p| ListPlaylistsEntry { name: p })
		.collect();

	Ok(Json(playlists))
}

#[derive(Serialize, Deserialize)]
pub struct SavePlaylistInput {
	pub tracks: Vec<String>,
}

#[put("/playlist/<name>", data = "<playlist>")]
fn save_playlist(
	db: State<'_, Arc<DB>>,
	auth: Auth,
	name: String,
	playlist: Json<SavePlaylistInput>,
) -> Result<()> {
	playlist::save_playlist(&name, &auth.username, &playlist.tracks, db.deref().deref())?;
	Ok(())
}

#[get("/playlist/<name>")]
fn read_playlist(
	db: State<'_, Arc<DB>>,
	auth: Auth,
	name: String,
) -> Result<Json<Vec<index::Song>>> {
	let songs = playlist::read_playlist(&name, &auth.username, db.deref().deref())?;
	Ok(Json(songs))
}

#[delete("/playlist/<name>")]
fn delete_playlist(db: State<'_, Arc<DB>>, auth: Auth, name: String) -> Result<()> {
	playlist::delete_playlist(&name, &auth.username, db.deref().deref())?;
	Ok(())
}

#[put("/lastfm/now_playing/<path>")]
fn lastfm_now_playing(db: State<'_, Arc<DB>>, auth: Auth, path: VFSPathBuf) -> Result<()> {
	if user::is_lastfm_linked(db.deref().deref(), &auth.username) {
		lastfm::now_playing(db.deref().deref(), &auth.username, &path.into() as &PathBuf)?;
	}
	Ok(())
}

#[post("/lastfm/scrobble/<path>")]
fn lastfm_scrobble(db: State<'_, Arc<DB>>, auth: Auth, path: VFSPathBuf) -> Result<()> {
	if user::is_lastfm_linked(db.deref().deref(), &auth.username) {
		lastfm::scrobble(db.deref().deref(), &auth.username, &path.into() as &PathBuf)?;
	}
	Ok(())
}

#[get("/lastfm/link?<token>&<content>")]
fn lastfm_link(
	db: State<'_, Arc<DB>>,
	auth: Auth,
	token: String,
	content: String,
) -> Result<Html<String>> {
	lastfm::link(db.deref().deref(), &auth.username, &token)?;

	// Percent decode
	let base64_content = RawStr::from_str(&content).percent_decode()?;

	// Base64 decode
	let popup_content = base64::decode(base64_content.as_bytes())?;

	// UTF-8 decode
	let popup_content_string = str::from_utf8(&popup_content)?;

	Ok(Html(popup_content_string.to_string()))
}

#[delete("/lastfm/link")]
fn lastfm_unlink(db: State<'_, Arc<DB>>, auth: Auth) -> Result<()> {
	lastfm::unlink(db.deref().deref(), &auth.username)?;
	Ok(())
}