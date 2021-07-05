use anyhow::*;
use core::ops::Deref;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{self, Path, PathBuf};

use crate::db::mount_points;

mod manager;
#[cfg(test)]
mod test;

pub use manager::*;

#[derive(Clone, Debug, Deserialize, Insertable, PartialEq, Queryable, Serialize)]
#[table_name = "mount_points"]
pub struct MountDir {
	pub source: String,
	pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Mount {
	pub source: PathBuf,
	pub name: String,
}

impl From<MountDir> for Mount {
	fn from(m: MountDir) -> Self {
		let separator_regex = Regex::new(r"\\|/").unwrap();
		let mut correct_separator = String::new();
		correct_separator.push(path::MAIN_SEPARATOR);
		let path_string = separator_regex.replace_all(&m.source, correct_separator.as_str());
		let source = PathBuf::from(path_string.deref());
		Self {
			name: m.name,
			source,
		}
	}
}

#[allow(clippy::upper_case_acronyms)]
pub struct VFS {
	mounts: Vec<Mount>,
}

impl VFS {
	pub fn new(mounts: Vec<Mount>) -> VFS {
		VFS { mounts }
	}

	pub fn real_to_virtual<P: AsRef<Path>>(&self, real_path: P) -> Result<PathBuf> {
		for mount in &self.mounts {
			if let Ok(p) = real_path.as_ref().strip_prefix(&mount.source) {
				let mount_path = Path::new(&mount.name);
				return if p.components().count() == 0 {
					Ok(mount_path.to_path_buf())
				} else {
					Ok(mount_path.join(p))
				};
			}
		}
		bail!("Real path has no match in VFS")
	}

	pub fn virtual_to_real<P: AsRef<Path>>(&self, virtual_path: P) -> Result<PathBuf> {
		for mount in &self.mounts {
			let mount_path = Path::new(&mount.name);
			if let Ok(p) = virtual_path.as_ref().strip_prefix(mount_path) {
				return if p.components().count() == 0 {
					Ok(mount.source.clone())
				} else {
					Ok(mount.source.join(p))
				};
			}
		}
		bail!("Virtual path has no match in VFS")
	}

	pub fn mounts(&self) -> &Vec<Mount> {
		&self.mounts
	}
}
