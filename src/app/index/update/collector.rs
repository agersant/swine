use crossbeam_channel::{Receiver, Sender};
use log::error;
use regex::Regex;

use super::*;

pub struct Collector {
	receiver: Receiver<traverser::Directory>,
	sender: Sender<inserter::Item>,
	album_art_pattern: Regex,
}

impl Collector {
	pub fn new(
		receiver: Receiver<traverser::Directory>,
		sender: Sender<inserter::Item>,
		album_art_pattern: Regex,
	) -> Self {
		Self {
			receiver,
			sender,
			album_art_pattern,
		}
	}

	pub fn collect(&self) {
		while let Ok(directory) = self.receiver.recv() {
			self.collect_directory(directory);
		}
	}

	fn collect_directory(&self, directory: traverser::Directory) {
		let mut directory_album = None;
		let mut directory_year = None;
		let mut directory_artist = None;
		let mut inconsistent_directory_album = false;
		let mut inconsistent_directory_year = false;
		let mut inconsistent_directory_artist = false;

		let directory_artwork = self.get_artwork(&directory);
		let directory_path_string = directory.path.to_string_lossy().to_string();
		let directory_parent_string = directory.parent.map(|p| p.to_string_lossy().to_string());

		for song in directory.songs {
			let tags = song.metadata;
			let path_string = song.path.to_string_lossy().to_string();

			if tags.year.is_some() {
				inconsistent_directory_year |=
					directory_year.is_some() && directory_year != tags.year;
				directory_year = tags.year;
			}

			if tags.album.is_some() {
				inconsistent_directory_album |=
					directory_album.is_some() && directory_album != tags.album;
				directory_album = tags.album.as_ref().cloned();
			}

			if tags.album_artist.is_some() {
				inconsistent_directory_artist |=
					directory_artist.is_some() && directory_artist != tags.album_artist;
				directory_artist = tags.album_artist.as_ref().cloned();
			} else if tags.artist.is_some() {
				inconsistent_directory_artist |=
					directory_artist.is_some() && directory_artist != tags.artist;
				directory_artist = tags.artist.as_ref().cloned();
			}

			let artwork_path = if tags.has_artwork {
				Some(path_string.clone())
			} else {
				directory_artwork.as_ref().cloned()
			};

			if let Err(e) = self.sender.send(inserter::Item::Song(inserter::Song {
				path: path_string,
				parent: directory_path_string.clone(),
				disc_number: tags.disc_number.map(|n| n as i32),
				track_number: tags.track_number.map(|n| n as i32),
				title: tags.title,
				duration: tags.duration.map(|n| n as i32),
				artist: tags.artist,
				album_artist: tags.album_artist,
				album: tags.album,
				year: tags.year,
				artwork: artwork_path,
				lyricist: tags.lyricist,
				composer: tags.composer,
				genre: tags.genre,
				label: tags.label,
			})) {
				error!("Error while sending song from collector: {}", e);
			}
		}

		if inconsistent_directory_year {
			directory_year = None;
		}
		if inconsistent_directory_album {
			directory_album = None;
		}
		if inconsistent_directory_artist {
			directory_artist = None;
		}

		if let Err(e) = self
			.sender
			.send(inserter::Item::Directory(inserter::Directory {
				path: directory_path_string,
				parent: directory_parent_string,
				artwork: directory_artwork,
				album: directory_album,
				artist: directory_artist,
				year: directory_year,
				date_added: directory.created,
			})) {
			error!("Error while sending directory from collector: {}", e);
		}
	}

	fn get_artwork(&self, directory: &traverser::Directory) -> Option<String> {
		let regex_artwork = directory.other_files.iter().find_map(|path| {
			let matches = path
				.file_name()
				.and_then(|n| n.to_str())
				.map(|n| self.album_art_pattern.is_match(n))
				.unwrap_or(false);
			if matches {
				Some(path.to_string_lossy().to_string())
			} else {
				None
			}
		});

		let embedded_artwork = directory.songs.iter().find_map(|song| {
			if song.metadata.has_artwork {
				Some(song.path.to_string_lossy().to_string())
			} else {
				None
			}
		});

		regex_artwork.or(embedded_artwork)
	}
}
