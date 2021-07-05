use anyhow::Result;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
	pub lastfm_username: Option<String>,
	pub web_theme_base: Option<String>,
	pub web_theme_accent: Option<String>,
}

impl Manager {
	pub fn read_preferences(&self, username: &str) -> Result<Preferences> {
		use self::users::dsl::*;
		let connection = self.db.connect()?;
		let (theme_base, theme_accent, read_lastfm_username) = users
			.select((web_theme_base, web_theme_accent, lastfm_username))
			.filter(name.eq(username))
			.get_result(&connection)?;
		Ok(Preferences {
			web_theme_base: theme_base,
			web_theme_accent: theme_accent,
			lastfm_username: read_lastfm_username,
		})
	}

	pub fn write_preferences(&self, username: &str, preferences: &Preferences) -> Result<()> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect()?;
		diesel::update(users.filter(name.eq(username)))
			.set((
				web_theme_base.eq(&preferences.web_theme_base),
				web_theme_accent.eq(&preferences.web_theme_accent),
			))
			.execute(&connection)?;
		Ok(())
	}
}
