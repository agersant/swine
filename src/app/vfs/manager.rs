use anyhow::*;
use diesel::prelude::*;

use super::*;
use crate::db::mount_points;
use crate::db::DB;

#[derive(Clone)]
pub struct Manager {
	db: DB,
}

impl Manager {
	pub fn new(db: DB) -> Self {
		Self { db }
	}

	pub fn get_vfs(&self) -> Result<VFS> {
		let mount_dirs = self.mount_dirs()?;
		let mounts = mount_dirs.into_iter().map(|p| p.into()).collect();
		Ok(VFS::new(mounts))
	}

	pub fn mount_dirs(&self) -> Result<Vec<MountDir>> {
		use self::mount_points::dsl::*;
		let connection = self.db.connect()?;
		let mount_dirs: Vec<MountDir> = mount_points
			.select((source, name))
			.get_results(&connection)?;
		Ok(mount_dirs)
	}

	pub fn set_mount_dirs(&self, mount_dirs: &[MountDir]) -> Result<()> {
		use self::mount_points::dsl::*;
		let connection = self.db.connect()?;
		diesel::delete(mount_points).execute(&connection)?;
		diesel::insert_into(mount_points)
			.values(mount_dirs)
			.execute(&*connection)?; // TODO https://github.com/diesel-rs/diesel/issues/1822
		Ok(())
	}
}
