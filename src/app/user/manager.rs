use anyhow::anyhow;
use diesel::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::app::settings::AuthSecret;
use crate::db::DB;

const HASH_ITERATIONS: u32 = 10000;

#[derive(Clone)]
pub struct Manager {
	// TODO make this private and move preferences methods in this file
	pub db: DB,
	auth_secret: AuthSecret,
}

impl Manager {
	pub fn new(db: DB, auth_secret: AuthSecret) -> Self {
		Self { db, auth_secret }
	}

	pub fn create(&self, new_user: &NewUser) -> Result<(), Error> {
		if new_user.name.is_empty() {
			return Err(Error::EmptyUsername);
		}

		let password_hash = hash_password(&new_user.password)?;
		let connection = self.db.connect()?;
		let new_user = User {
			name: new_user.name.to_owned(),
			password_hash,
			admin: new_user.admin as i32,
		};

		diesel::insert_into(users::table)
			.values(&new_user)
			.execute(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub fn delete(&self, username: &str) -> Result<(), Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		diesel::delete(users.filter(name.eq(username)))
			.execute(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub fn set_password(&self, username: &str, password: &str) -> Result<(), Error> {
		let hash = hash_password(password)?;
		let connection = self.db.connect()?;
		use crate::db::users::dsl::*;
		diesel::update(users.filter(name.eq(username)))
			.set(password_hash.eq(hash))
			.execute(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub fn set_is_admin(&self, username: &str, is_admin: bool) -> Result<(), Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		diesel::update(users.filter(name.eq(username)))
			.set(admin.eq(is_admin as i32))
			.execute(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub fn login(&self, username: &str, password: &str) -> Result<AuthToken, Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		match users
			.select(password_hash)
			.filter(name.eq(username))
			.get_result(&connection)
		{
			Err(diesel::result::Error::NotFound) => Err(Error::IncorrectUsername),
			Ok(hash) => {
				let hash: String = hash;
				if verify_password(&hash, password) {
					let authorization = Authorization {
						username: username.to_owned(),
						scope: AuthorizationScope::PolarisAuth,
					};
					self.generate_auth_token(&authorization)
				} else {
					Err(Error::IncorrectPassword)
				}
			}
			Err(_) => Err(Error::Unspecified),
		}
	}

	pub fn authenticate(
		&self,
		auth_token: &AuthToken,
		scope: AuthorizationScope,
	) -> Result<Authorization, Error> {
		let authorization = self.decode_auth_token(auth_token, scope)?;
		if self.exists(&authorization.username)? {
			Ok(authorization)
		} else {
			Err(Error::IncorrectUsername)
		}
	}

	fn decode_auth_token(
		&self,
		auth_token: &AuthToken,
		scope: AuthorizationScope,
	) -> Result<Authorization, Error> {
		let AuthToken(data) = auth_token;
		let ttl = match scope {
			AuthorizationScope::PolarisAuth => 0,      // permanent
			AuthorizationScope::LastFMLink => 10 * 60, // 10 minutes
		};
		let authorization = branca::decode(data, &self.auth_secret.key, ttl)
			.map_err(|_| Error::InvalidAuthToken)?;
		let authorization: Authorization =
			serde_json::from_slice(&authorization[..]).map_err(|_| Error::InvalidAuthToken)?;
		if authorization.scope != scope {
			return Err(Error::IncorrectAuthorizationScope);
		}
		Ok(authorization)
	}

	fn generate_auth_token(&self, authorization: &Authorization) -> Result<AuthToken, Error> {
		let serialized_authorization =
			serde_json::to_string(&authorization).map_err(|_| Error::Unspecified)?;
		branca::encode(
			serialized_authorization.as_bytes(),
			&self.auth_secret.key,
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.map_err(|_| Error::Unspecified)?
				.as_secs() as u32,
		)
		.map_err(|_| Error::Unspecified)
		.map(AuthToken)
	}

	pub fn count(&self) -> anyhow::Result<i64> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		let count = users.count().get_result(&connection)?;
		Ok(count)
	}

	pub fn list(&self) -> Result<Vec<User>, Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		users
			.select((name, password_hash, admin))
			.get_results(&connection)
			.map_err(|_| Error::Unspecified)
	}

	pub fn exists(&self, username: &str) -> Result<bool, Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		let results: Vec<String> = users
			.select(name)
			.filter(name.eq(username))
			.get_results(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(!results.is_empty())
	}

	pub fn is_admin(&self, username: &str) -> Result<bool, Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		let is_admin: i32 = users
			.filter(name.eq(username))
			.select(admin)
			.get_result(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(is_admin != 0)
	}

	pub fn lastfm_link(
		&self,
		username: &str,
		lastfm_login: &str,
		session_key: &str,
	) -> Result<(), Error> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		diesel::update(users.filter(name.eq(username)))
			.set((
				lastfm_username.eq(lastfm_login),
				lastfm_session_key.eq(session_key),
			))
			.execute(&connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub fn generate_lastfm_link_token(&self, username: &str) -> Result<AuthToken, Error> {
		self.generate_auth_token(&Authorization {
			username: username.to_owned(),
			scope: AuthorizationScope::LastFMLink,
		})
	}

	pub fn get_lastfm_session_key(&self, username: &str) -> anyhow::Result<String> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		let token = users
			.filter(name.eq(username))
			.select(lastfm_session_key)
			.get_result(&connection)?;
		match token {
			Some(t) => Ok(t),
			_ => Err(anyhow!("Missing LastFM credentials")),
		}
	}

	pub fn is_lastfm_linked(&self, username: &str) -> bool {
		self.get_lastfm_session_key(username).is_ok()
	}

	pub fn lastfm_unlink(&self, username: &str) -> anyhow::Result<()> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		let null: Option<String> = None;
		diesel::update(users.filter(name.eq(username)))
			.set((lastfm_session_key.eq(&null), lastfm_username.eq(&null)))
			.execute(&connection)?;
		Ok(())
	}
}

fn hash_password(password: &str) -> Result<String, Error> {
	if password.is_empty() {
		return Err(Error::EmptyPassword);
	}
	pbkdf2::pbkdf2_simple(password, HASH_ITERATIONS).map_err(|_| Error::Unspecified)
}

fn verify_password(password_hash: &str, attempted_password: &str) -> bool {
	pbkdf2::pbkdf2_check(attempted_password, password_hash).is_ok()
}
