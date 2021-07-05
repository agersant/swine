use anyhow::*;
use crossbeam_channel::Receiver;
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
	pub lyricist: Option<String>,
	pub composer: Option<String>,
	pub genre: Option<String>,
	pub label: Option<String>,
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
		let new_directories = Vec::with_capacity(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		let new_songs = Vec::with_capacity(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		Self {
			receiver,
			new_directories,
			new_songs,
			db,
		}
	}

	pub fn insert(&mut self) {
		while let Ok(item) = self.receiver.recv() {
			self.insert_item(item);
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
		let res = self.db.connect().and_then(|connection| {
			diesel::insert_into(directories::table)
				.values(&self.new_directories)
				.execute(&*connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
				.map_err(Error::new)
		});
		if res.is_err() {
			error!("Could not insert new directories in database");
		}
		self.new_directories.clear();
	}

	fn flush_songs(&mut self) {
		let res = self.db.connect().and_then(|connection| {
			diesel::insert_into(songs::table)
				.values(&self.new_songs)
				.execute(&*connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
				.map_err(Error::new)
		});
		if res.is_err() {
			error!("Could not insert new songs in database");
		}
		self.new_songs.clear();
	}
}

impl Drop for Inserter {
	fn drop(&mut self) {
		if !self.new_directories.is_empty() {
			self.flush_directories();
		}
		if !self.new_songs.is_empty() {
			self.flush_songs();
		}
	}
}
