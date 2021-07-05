use actix_files::NamedFile;
use actix_web::{
	client::HttpError,
	delete,
	dev::{MessageBody, Payload, Service, ServiceRequest, ServiceResponse},
	error::{BlockingError, ErrorForbidden, ErrorInternalServerError, ErrorUnauthorized},
	get,
	http::StatusCode,
	post, put,
	web::{self, Data, Json, JsonConfig, ServiceConfig},
	FromRequest, HttpMessage, HttpRequest, HttpResponse, ResponseError,
};
use actix_web_httpauth::extractors::{basic::BasicAuth, bearer::BearerAuth};
use cookie::{self, *};
use futures_util::future::{err, ok};
use percent_encoding::percent_decode_str;
use std::future::Future;
use std::ops::Deref;
use std::path::Path;
use std::pin::Pin;
use std::str;

use crate::app::{
	config, ddns,
	index::{self, Index},
	lastfm, playlist, settings, thumbnail, user,
	vfs::{self, MountDir},
};
use crate::service::{dto, error::*};

pub fn make_config() -> impl FnOnce(&mut ServiceConfig) + Clone {
	move |cfg: &mut ServiceConfig| {
		let megabyte = 1024 * 1024;
		cfg.app_data(JsonConfig::default().limit(4 * megabyte)) // 4MB
			.service(version)
			.service(initial_setup)
			.service(apply_config)
			.service(get_settings)
			.service(put_settings)
			.service(list_mount_dirs)
			.service(put_mount_dirs)
			.service(get_ddns_config)
			.service(put_ddns_config)
			.service(list_users)
			.service(create_user)
			.service(update_user)
			.service(delete_user)
			.service(get_preferences)
			.service(put_preferences)
			.service(trigger_index)
			.service(login)
			.service(browse_root)
			.service(browse)
			.service(flatten_root)
			.service(flatten)
			.service(random)
			.service(recent)
			.service(search_root)
			.service(search)
			.service(get_audio)
			.service(get_thumbnail)
			.service(list_playlists)
			.service(save_playlist)
			.service(read_playlist)
			.service(delete_playlist)
			.service(lastfm_now_playing)
			.service(lastfm_scrobble)
			.service(lastfm_link_token)
			.service(lastfm_link)
			.service(lastfm_unlink);
	}
}

impl ResponseError for APIError {
	fn status_code(&self) -> StatusCode {
		match self {
			APIError::IncorrectCredentials => StatusCode::UNAUTHORIZED,
			APIError::EmptyUsername => StatusCode::BAD_REQUEST,
			APIError::EmptyPassword => StatusCode::BAD_REQUEST,
			APIError::DeletingOwnAccount => StatusCode::CONFLICT,
			APIError::OwnAdminPrivilegeRemoval => StatusCode::CONFLICT,
			APIError::AudioFileIOError => StatusCode::NOT_FOUND,
			APIError::ThumbnailFileIOError => StatusCode::NOT_FOUND,
			APIError::LastFMAccountNotLinked => StatusCode::NO_CONTENT,
			APIError::LastFMLinkContentBase64DecodeError => StatusCode::BAD_REQUEST,
			APIError::LastFMLinkContentEncodingError => StatusCode::BAD_REQUEST,
			APIError::UserNotFound => StatusCode::NOT_FOUND,
			APIError::PlaylistNotFound => StatusCode::NOT_FOUND,
			APIError::VFSPathNotFound => StatusCode::NOT_FOUND,
			APIError::Unspecified => StatusCode::INTERNAL_SERVER_ERROR,
		}
	}
}

#[derive(Clone)]
struct Cookies {
	jar: CookieJar,
	key: Key,
}

impl Cookies {
	fn new(key: Key) -> Self {
		let jar = CookieJar::new();
		Self { jar, key }
	}

	fn add_original(&mut self, cookie: Cookie<'static>) {
		self.jar.add_original(cookie);
	}

	fn add(&mut self, cookie: Cookie<'static>) {
		self.jar.add(cookie);
	}

	fn add_signed(&mut self, cookie: Cookie<'static>) {
		self.jar.signed(&self.key).add(cookie);
	}

	#[allow(dead_code)]
	fn get(&self, name: &str) -> Option<&Cookie> {
		self.jar.get(name)
	}

	fn get_signed(&mut self, name: &str) -> Option<Cookie> {
		self.jar.signed(&self.key).get(name)
	}
}

impl FromRequest for Cookies {
	type Error = actix_web::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;
	type Config = ();

	fn from_request(request: &HttpRequest, _payload: &mut Payload) -> Self::Future {
		let request_cookies = match request.cookies() {
			Ok(c) => c,
			Err(_) => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
		};

		let key = match request.app_data::<Data<Key>>() {
			Some(k) => k.as_ref(),
			None => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
		};

		let mut cookies = Cookies::new(key.clone());
		for cookie in request_cookies.deref() {
			cookies.add_original(cookie.clone());
		}

		Box::pin(ok(cookies))
	}
}

#[derive(Debug)]
enum AuthSource {
	AuthorizationBasic,
	AuthorizationBearer,
	Cookie,
	QueryParameter,
}

#[derive(Debug)]
struct Auth {
	username: String,
	source: AuthSource,
}

impl FromRequest for Auth {
	type Error = actix_web::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;
	type Config = ();

	fn from_request(request: &HttpRequest, payload: &mut Payload) -> Self::Future {
		let user_manager = match request.app_data::<Data<user::Manager>>() {
			Some(m) => m.clone(),
			None => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
		};

		let cookies_future = Cookies::from_request(request, payload);
		let basic_auth_future = BasicAuth::from_request(request, payload);
		let bearer_auth_future = BearerAuth::from_request(request, payload);
		let query_params_future =
			web::Query::<dto::AuthQueryParameters>::from_request(request, payload);

		Box::pin(async move {
			// Auth via session cookie
			{
				let mut cookies = cookies_future.await?;
				if let Some(session_cookie) = cookies.get_signed(dto::COOKIE_SESSION) {
					let username = session_cookie.value().to_string();
					let exists = block(move || user_manager.exists(&username)).await?;
					if !exists {
						return Err(ErrorUnauthorized(APIError::Unspecified));
					}
					return Ok(Auth {
						username: session_cookie.value().to_string(),
						source: AuthSource::Cookie,
					});
				}
			}

			// Auth via bearer token in query parameter
			if let Ok(query) = query_params_future.await {
				let auth_token = user::AuthToken(query.auth_token.clone());
				let authorization = block(move || {
					user_manager.authenticate(&auth_token, user::AuthorizationScope::PolarisAuth)
				})
				.await?;
				return Ok(Auth {
					username: authorization.username,
					source: AuthSource::QueryParameter,
				});
			}

			// Auth via bearer token in authorization header
			if let Ok(bearer_auth) = bearer_auth_future.await {
				let auth_token = user::AuthToken(bearer_auth.token().to_owned());
				let authorization = block(move || {
					user_manager.authenticate(&auth_token, user::AuthorizationScope::PolarisAuth)
				})
				.await?;
				return Ok(Auth {
					username: authorization.username,
					source: AuthSource::AuthorizationBearer,
				});
			}

			// Auth via basic authorization header
			{
				let basic_auth = basic_auth_future.await?;
				let username = basic_auth.user_id().to_string();
				let password = basic_auth
					.password()
					.map(|s| s.as_ref())
					.unwrap_or("")
					.to_string();
				let auth_result = block(move || user_manager.login(&username, &password)).await;
				if auth_result.is_ok() {
					Ok(Auth {
						username: basic_auth.user_id().to_string(),
						source: AuthSource::AuthorizationBasic,
					})
				} else {
					Err(ErrorUnauthorized(APIError::Unspecified))
				}
			}
		})
	}
}

#[derive(Debug)]
struct AdminRights {
	auth: Option<Auth>,
}

impl FromRequest for AdminRights {
	type Error = actix_web::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;
	type Config = ();

	fn from_request(request: &HttpRequest, payload: &mut Payload) -> Self::Future {
		let user_manager = match request.app_data::<Data<user::Manager>>() {
			Some(m) => m.clone(),
			None => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
		};

		let auth_future = Auth::from_request(request, payload);

		Box::pin(async move {
			let user_manager_count = user_manager.clone();
			let user_count = block(move || user_manager_count.count()).await;
			match user_count {
				Err(_) => return Err(ErrorInternalServerError(APIError::Unspecified)),
				Ok(0) => return Ok(AdminRights { auth: None }),
				_ => (),
			};

			let auth = auth_future.await?;
			let username = auth.username.clone();
			let is_admin = block(move || user_manager.is_admin(&username)).await?;
			if is_admin {
				Ok(AdminRights { auth: Some(auth) })
			} else {
				Err(ErrorForbidden(APIError::Unspecified))
			}
		})
	}
}

pub fn http_auth_middleware<
	B: MessageBody + 'static,
	S: Service<Response = ServiceResponse<B>, Request = ServiceRequest, Error = actix_web::Error>
		+ 'static,
>(
	request: ServiceRequest,
	service: &mut S,
) -> Pin<Box<dyn Future<Output = Result<ServiceResponse<B>, actix_web::Error>>>> {
	let user_manager = match request.app_data::<Data<user::Manager>>() {
		Some(m) => m.clone(),
		None => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
	};

	let (request, mut payload) = request.into_parts();
	let auth_future = Auth::from_request(&request, &mut payload);
	let cookies_future = Cookies::from_request(&request, &mut payload);
	let request = match ServiceRequest::from_parts(request, payload) {
		Ok(s) => s,
		Err(_) => return Box::pin(err(ErrorInternalServerError(APIError::Unspecified))),
	};

	let response_future = service.call(request);
	Box::pin(async move {
		let mut response = response_future.await?;
		if let Ok(auth) = auth_future.await {
			let set_cookies = match auth.source {
				AuthSource::AuthorizationBasic => true,
				AuthSource::AuthorizationBearer => false,
				AuthSource::Cookie => false,
				AuthSource::QueryParameter => false,
			};
			if set_cookies {
				let cookies = cookies_future.await?;
				let username = auth.username.clone();
				let is_admin = block(move || {
					user_manager
						.is_admin(&auth.username)
						.map_err(|_| APIError::Unspecified)
				})
				.await?;
				add_auth_cookies(response.response_mut(), &cookies, &username, is_admin)?;
			}
		}
		Ok(response)
	})
}

fn add_auth_cookies<T>(
	response: &mut HttpResponse<T>,
	cookies: &Cookies,
	username: &str,
	is_admin: bool,
) -> Result<(), HttpError> {
	let mut cookies = cookies.clone();

	cookies.add_signed(
		Cookie::build(dto::COOKIE_SESSION, username.to_owned())
			.same_site(cookie::SameSite::Lax)
			.http_only(true)
			.permanent()
			.finish(),
	);

	cookies.add(
		Cookie::build(dto::COOKIE_USERNAME, username.to_owned())
			.same_site(cookie::SameSite::Lax)
			.http_only(false)
			.permanent()
			.path("/")
			.finish(),
	);

	cookies.add(
		Cookie::build(dto::COOKIE_ADMIN, format!("{}", is_admin))
			.same_site(cookie::SameSite::Lax)
			.http_only(false)
			.permanent()
			.path("/")
			.finish(),
	);

	let headers = response.headers_mut();
	for cookie in cookies.jar.delta() {
		http::HeaderValue::from_str(&cookie.to_string()).map(|c| {
			headers.append(http::header::SET_COOKIE, c);
		})?;
	}

	Ok(())
}

async fn block<F, I, E>(f: F) -> Result<I, APIError>
where
	F: FnOnce() -> Result<I, E> + Send + 'static,
	I: Send + 'static,
	E: Send + std::fmt::Debug + 'static + Into<APIError>,
{
	actix_web::web::block(f).await.map_err(|e| match e {
		BlockingError::Error(e) => e.into(),
		BlockingError::Canceled => APIError::Unspecified,
	})
}

#[get("/version")]
async fn version() -> Json<dto::Version> {
	let current_version = dto::Version {
		major: dto::API_MAJOR_VERSION,
		minor: dto::API_MINOR_VERSION,
	};
	Json(current_version)
}

#[get("/initial_setup")]
async fn initial_setup(
	user_manager: Data<user::Manager>,
) -> Result<Json<dto::InitialSetup>, APIError> {
	let initial_setup = block(move || -> Result<dto::InitialSetup, APIError> {
		let users = user_manager.list()?;
		let has_any_admin = users.iter().any(|u| u.is_admin());
		Ok(dto::InitialSetup {
			has_any_users: has_any_admin,
		})
	})
	.await?;
	Ok(Json(initial_setup))
}

#[put("/config")]
async fn apply_config(
	_admin_rights: AdminRights,
	config_manager: Data<config::Manager>,
	config: Json<dto::Config>,
) -> Result<HttpResponse, APIError> {
	block(move || config_manager.apply(&config.to_owned().into())).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/settings")]
async fn get_settings(
	settings_manager: Data<settings::Manager>,
	_admin_rights: AdminRights,
) -> Result<Json<dto::Settings>, APIError> {
	let settings = block(move || settings_manager.read()).await?;
	Ok(Json(settings.into()))
}

#[put("/settings")]
async fn put_settings(
	_admin_rights: AdminRights,
	settings_manager: Data<settings::Manager>,
	new_settings: Json<dto::NewSettings>,
) -> Result<HttpResponse, APIError> {
	block(move || settings_manager.amend(&new_settings.to_owned().into())).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/mount_dirs")]
async fn list_mount_dirs(
	vfs_manager: Data<vfs::Manager>,
	_admin_rights: AdminRights,
) -> Result<Json<Vec<dto::MountDir>>, APIError> {
	let mount_dirs = block(move || vfs_manager.mount_dirs()).await?;
	let mount_dirs = mount_dirs.into_iter().map(|m| m.into()).collect();
	Ok(Json(mount_dirs))
}

#[put("/mount_dirs")]
async fn put_mount_dirs(
	_admin_rights: AdminRights,
	vfs_manager: Data<vfs::Manager>,
	new_mount_dirs: Json<Vec<dto::MountDir>>,
) -> Result<HttpResponse, APIError> {
	let new_mount_dirs: Vec<MountDir> = new_mount_dirs
		.to_owned()
		.into_iter()
		.map(|m| m.into())
		.collect();
	block(move || vfs_manager.set_mount_dirs(&new_mount_dirs)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/ddns")]
async fn get_ddns_config(
	ddns_manager: Data<ddns::Manager>,
	_admin_rights: AdminRights,
) -> Result<Json<dto::DDNSConfig>, APIError> {
	let ddns_config = block(move || ddns_manager.config()).await?;
	Ok(Json(ddns_config.into()))
}

#[put("/ddns")]
async fn put_ddns_config(
	_admin_rights: AdminRights,
	ddns_manager: Data<ddns::Manager>,
	new_ddns_config: Json<dto::DDNSConfig>,
) -> Result<HttpResponse, APIError> {
	block(move || ddns_manager.set_config(&new_ddns_config.to_owned().into())).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/users")]
async fn list_users(
	user_manager: Data<user::Manager>,
	_admin_rights: AdminRights,
) -> Result<Json<Vec<dto::User>>, APIError> {
	let users = block(move || user_manager.list()).await?;
	let users = users.into_iter().map(|u| u.into()).collect();
	Ok(Json(users))
}

#[post("/user")]
async fn create_user(
	user_manager: Data<user::Manager>,
	_admin_rights: AdminRights,
	new_user: Json<dto::NewUser>,
) -> Result<HttpResponse, APIError> {
	let new_user = new_user.to_owned().into();
	block(move || user_manager.create(&new_user)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[put("/user/{name}")]
async fn update_user(
	user_manager: Data<user::Manager>,
	admin_rights: AdminRights,
	name: web::Path<String>,
	user_update: Json<dto::UserUpdate>,
) -> Result<HttpResponse, APIError> {
	if let Some(auth) = &admin_rights.auth {
		if auth.username == name.as_str() && user_update.new_is_admin == Some(false) {
			return Err(APIError::OwnAdminPrivilegeRemoval);
		}
	}

	block(move || -> Result<(), APIError> {
		if let Some(password) = &user_update.new_password {
			user_manager.set_password(&name, password)?;
		}
		if let Some(is_admin) = &user_update.new_is_admin {
			user_manager.set_is_admin(&name, *is_admin)?;
		}
		Ok(())
	})
	.await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[delete("/user/{name}")]
async fn delete_user(
	user_manager: Data<user::Manager>,
	admin_rights: AdminRights,
	name: web::Path<String>,
) -> Result<HttpResponse, APIError> {
	if let Some(auth) = &admin_rights.auth {
		if auth.username == name.as_str() {
			return Err(APIError::DeletingOwnAccount);
		}
	}
	block(move || user_manager.delete(&name)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/preferences")]
async fn get_preferences(
	user_manager: Data<user::Manager>,
	auth: Auth,
) -> Result<Json<user::Preferences>, APIError> {
	let preferences = block(move || user_manager.read_preferences(&auth.username)).await?;
	Ok(Json(preferences))
}

#[put("/preferences")]
async fn put_preferences(
	user_manager: Data<user::Manager>,
	auth: Auth,
	preferences: Json<user::Preferences>,
) -> Result<HttpResponse, APIError> {
	block(move || user_manager.write_preferences(&auth.username, &preferences)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[post("/trigger_index")]
async fn trigger_index(
	index: Data<Index>,
	_admin_rights: AdminRights,
) -> Result<HttpResponse, APIError> {
	index.trigger_reindex();
	Ok(HttpResponse::new(StatusCode::OK))
}

#[post("/auth")]
async fn login(
	user_manager: Data<user::Manager>,
	credentials: Json<dto::Credentials>,
	cookies: Cookies,
) -> Result<HttpResponse, APIError> {
	let username = credentials.username.clone();
	let (user::AuthToken(token), is_admin) =
		block(move || -> Result<(user::AuthToken, bool), APIError> {
			let auth_token = user_manager.login(&credentials.username, &credentials.password)?;
			let is_admin = user_manager.is_admin(&credentials.username)?;
			Ok((auth_token, is_admin))
		})
		.await?;
	let authorization = dto::Authorization {
		username: username.clone(),
		token,
		is_admin,
	};
	let mut response = HttpResponse::Ok().json(authorization);
	add_auth_cookies(&mut response, &cookies, &username, is_admin)
		.map_err(|_| APIError::Unspecified)?;
	Ok(response)
}

#[get("/browse")]
async fn browse_root(
	index: Data<Index>,
	_auth: Auth,
) -> Result<Json<Vec<index::CollectionFile>>, APIError> {
	let result = block(move || index.browse(Path::new(""))).await?;
	Ok(Json(result))
}

#[get("/browse/{path:.*}")]
async fn browse(
	index: Data<Index>,
	_auth: Auth,
	path: web::Path<String>,
) -> Result<Json<Vec<index::CollectionFile>>, APIError> {
	let result = block(move || {
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		index.browse(Path::new(path.as_ref()))
	})
	.await?;
	Ok(Json(result))
}

#[get("/flatten")]
async fn flatten_root(index: Data<Index>, _auth: Auth) -> Result<Json<Vec<index::Song>>, APIError> {
	let songs = block(move || index.flatten(Path::new(""))).await?;
	Ok(Json(songs))
}

#[get("/flatten/{path:.*}")]
async fn flatten(
	index: Data<Index>,
	_auth: Auth,
	path: web::Path<String>,
) -> Result<Json<Vec<index::Song>>, APIError> {
	let songs = block(move || {
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		index.flatten(Path::new(path.as_ref()))
	})
	.await?;
	Ok(Json(songs))
}

#[get("/random")]
async fn random(index: Data<Index>, _auth: Auth) -> Result<Json<Vec<index::Directory>>, APIError> {
	let result = block(move || index.get_random_albums(20)).await?;
	Ok(Json(result))
}

#[get("/recent")]
async fn recent(index: Data<Index>, _auth: Auth) -> Result<Json<Vec<index::Directory>>, APIError> {
	let result = block(move || index.get_recent_albums(20)).await?;
	Ok(Json(result))
}

#[get("/search")]
async fn search_root(
	index: Data<Index>,
	_auth: Auth,
) -> Result<Json<Vec<index::CollectionFile>>, APIError> {
	let result = block(move || index.search("")).await?;
	Ok(Json(result))
}

#[get("/search/{query:.*}")]
async fn search(
	index: Data<Index>,
	_auth: Auth,
	query: web::Path<String>,
) -> Result<Json<Vec<index::CollectionFile>>, APIError> {
	let result = block(move || index.search(&query)).await?;
	Ok(Json(result))
}

#[get("/audio/{path:.*}")]
async fn get_audio(
	vfs_manager: Data<vfs::Manager>,
	_auth: Auth,
	path: web::Path<String>,
) -> Result<NamedFile, APIError> {
	let audio_path = block(move || {
		let vfs = vfs_manager.get_vfs()?;
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		vfs.virtual_to_real(Path::new(path.as_ref()))
			.map_err(|_| APIError::VFSPathNotFound)
	})
	.await?;

	let named_file = NamedFile::open(&audio_path).map_err(|_| APIError::AudioFileIOError)?;
	Ok(named_file)
}

#[get("/thumbnail/{path:.*}")]
async fn get_thumbnail(
	vfs_manager: Data<vfs::Manager>,
	thumbnails_manager: Data<thumbnail::Manager>,
	_auth: Auth,
	path: web::Path<String>,
	options_input: web::Query<dto::ThumbnailOptions>,
) -> Result<NamedFile, APIError> {
	let options = thumbnail::Options::from(options_input.0);

	let thumbnail_path = block(move || {
		let vfs = vfs_manager.get_vfs()?;
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		let image_path = vfs
			.virtual_to_real(Path::new(path.as_ref()))
			.map_err(|_| APIError::VFSPathNotFound)?;
		thumbnails_manager
			.get_thumbnail(&image_path, &options)
			.map_err(|_| APIError::Unspecified)
	})
	.await?;

	let named_file =
		NamedFile::open(&thumbnail_path).map_err(|_| APIError::ThumbnailFileIOError)?;

	Ok(named_file)
}

#[get("/playlists")]
async fn list_playlists(
	playlist_manager: Data<playlist::Manager>,
	auth: Auth,
) -> Result<Json<Vec<dto::ListPlaylistsEntry>>, APIError> {
	let playlist_names = block(move || playlist_manager.list_playlists(&auth.username)).await?;
	let playlists: Vec<dto::ListPlaylistsEntry> = playlist_names
		.into_iter()
		.map(|p| dto::ListPlaylistsEntry { name: p })
		.collect();

	Ok(Json(playlists))
}

#[put("/playlist/{name}")]
async fn save_playlist(
	playlist_manager: Data<playlist::Manager>,
	auth: Auth,
	name: web::Path<String>,
	playlist: Json<dto::SavePlaylistInput>,
) -> Result<HttpResponse, APIError> {
	block(move || playlist_manager.save_playlist(&name, &auth.username, &playlist.tracks)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/playlist/{name}")]
async fn read_playlist(
	playlist_manager: Data<playlist::Manager>,
	auth: Auth,
	name: web::Path<String>,
) -> Result<Json<Vec<index::Song>>, APIError> {
	let songs = block(move || playlist_manager.read_playlist(&name, &auth.username)).await?;
	Ok(Json(songs))
}

#[delete("/playlist/{name}")]
async fn delete_playlist(
	playlist_manager: Data<playlist::Manager>,
	auth: Auth,
	name: web::Path<String>,
) -> Result<HttpResponse, APIError> {
	block(move || playlist_manager.delete_playlist(&name, &auth.username)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[put("/lastfm/now_playing/{path:.*}")]
async fn lastfm_now_playing(
	lastfm_manager: Data<lastfm::Manager>,
	user_manager: Data<user::Manager>,
	auth: Auth,
	path: web::Path<String>,
) -> Result<HttpResponse, APIError> {
	block(move || -> Result<(), APIError> {
		if !user_manager.is_lastfm_linked(&auth.username) {
			return Err(APIError::LastFMAccountNotLinked);
		}
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		lastfm_manager.now_playing(&auth.username, Path::new(path.as_ref()))?;
		Ok(())
	})
	.await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[post("/lastfm/scrobble/{path:.*}")]
async fn lastfm_scrobble(
	lastfm_manager: Data<lastfm::Manager>,
	user_manager: Data<user::Manager>,
	auth: Auth,
	path: web::Path<String>,
) -> Result<HttpResponse, APIError> {
	block(move || -> Result<(), APIError> {
		if !user_manager.is_lastfm_linked(&auth.username) {
			return Err(APIError::LastFMAccountNotLinked);
		}
		let path = percent_decode_str(&(path.0)).decode_utf8_lossy();
		lastfm_manager.scrobble(&auth.username, Path::new(path.as_ref()))?;
		Ok(())
	})
	.await?;
	Ok(HttpResponse::new(StatusCode::OK))
}

#[get("/lastfm/link_token")]
async fn lastfm_link_token(
	lastfm_manager: Data<lastfm::Manager>,
	auth: Auth,
) -> Result<Json<dto::LastFMLinkToken>, APIError> {
	let user::AuthToken(value) =
		block(move || lastfm_manager.generate_link_token(&auth.username)).await?;
	Ok(Json(dto::LastFMLinkToken { value }))
}

#[get("/lastfm/link")]
async fn lastfm_link(
	lastfm_manager: Data<lastfm::Manager>,
	user_manager: Data<user::Manager>,
	payload: web::Query<dto::LastFMLink>,
) -> Result<HttpResponse, APIError> {
	let popup_content_string = block(move || {
		let auth_token = user::AuthToken(payload.auth_token.clone());
		let authorization =
			user_manager.authenticate(&auth_token, user::AuthorizationScope::LastFMLink)?;
		let lastfm_token = &payload.token;
		lastfm_manager.link(&authorization.username, lastfm_token)?;

		// Percent decode
		let base64_content = percent_decode_str(&payload.content).decode_utf8_lossy();

		// Base64 decode
		let popup_content = base64::decode(base64_content.as_bytes())
			.map_err(|_| APIError::LastFMLinkContentBase64DecodeError)?;

		// UTF-8 decode
		str::from_utf8(&popup_content)
			.map_err(|_| APIError::LastFMLinkContentEncodingError)
			.map(|s| s.to_owned())
	})
	.await?;

	Ok(HttpResponse::build(StatusCode::OK)
		.content_type("text/html; charset=utf-8")
		.body(popup_content_string))
}

#[delete("/lastfm/link")]
async fn lastfm_unlink(
	lastfm_manager: Data<lastfm::Manager>,
	auth: Auth,
) -> Result<HttpResponse, APIError> {
	block(move || lastfm_manager.unlink(&auth.username)).await?;
	Ok(HttpResponse::new(StatusCode::OK))
}
