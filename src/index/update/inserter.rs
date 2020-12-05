use anyhow::*;
use crossbeam_channel::Receiver;
use diesel;
use diesel::prelude::*;
use log::error;

use crate::db::{directories, songs, DB};

const INDEX_BUILDING_INSERT_BUFFER_SIZE: usize = 1000; // Insertions in each transaction

#[derive(Debug, Insertable)]
#[table_name = "songs"]
pub struct Song {
	pub path: String,
	pub parent: String,
	pub track_number: Option<i32>,
	pub disc_number: Option<i32>,
	pub title: Option<String>,
	pub artist: Option<String>,
	pub album_artist: Option<String>,
	pub year: Option<i32>,
	pub album: Option<String>,
	pub artwork: Option<String>,
	pub duration: Option<i32>,
}

#[derive(Debug, Insertable)]
#[table_name = "directories"]
pub struct Directory {
	pub path: String,
	pub parent: Option<String>,
	pub artist: Option<String>,
	pub year: Option<i32>,
	pub album: Option<String>,
	pub artwork: Option<String>,
	pub date_added: i32,
}

pub enum Item {
	Directory(Directory),
	Song(Song),
}

pub struct Inserter {
	receiver: Receiver<Item>,
	new_directories: Vec<Directory>,
	new_songs: Vec<Song>,
	db: DB,
}

impl Inserter {
	pub fn new(db: DB, receiver: Receiver<Item>) -> Self {
		let mut new_directories = Vec::new();
		let mut new_songs = Vec::new();
		new_directories.reserve_exact(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		new_songs.reserve_exact(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		Self {
			db,
			receiver,
			new_directories,
			new_songs,
		}
	}

	pub fn insert(&mut self) {
		loop {
			match self.receiver.recv() {
				Ok(item) => self.insert_item(item),
				Err(_) => break,
			}
		}

		if self.new_directories.len() > 0 {
			self.flush_directories();
		}
		if self.new_songs.len() > 0 {
			self.flush_songs();
		}
	}

	fn insert_item(&mut self, insert: Item) {
		match insert {
			Item::Directory(d) => {
				self.new_directories.push(d);
				if self.new_directories.len() >= INDEX_BUILDING_INSERT_BUFFER_SIZE {
					self.flush_directories();
				}
			}
			Item::Song(s) => {
				self.new_songs.push(s);
				if self.new_songs.len() >= INDEX_BUILDING_INSERT_BUFFER_SIZE {
					self.flush_songs();
				}
			}
		};
	}

	fn flush_directories(&mut self) {
		if self
			.db
			.connect()
			.and_then(|connection| {
				diesel::insert_into(directories::table)
					.values(&self.new_directories)
					.execute(&*connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
					.map_err(Error::new)
			})
			.is_err()
		{
			error!("Could not insert new directories in database");
		}
		self.new_directories.clear();
	}

	fn flush_songs(&mut self) {
		if self
			.db
			.connect()
			.and_then(|connection| {
				diesel::insert_into(songs::table)
					.values(&self.new_songs)
					.execute(&*connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
					.map_err(Error::new)
			})
			.is_err()
		{
			error!("Could not insert new songs in database");
		}
		self.new_songs.clear();
	}
}
