use anyhow::*;
use lewton::inside_ogg::OggStreamReader;
use log::error;
use regex::Regex;
use std::fs;
use std::path::Path;

use crate::utils;
use crate::utils::AudioFormat;

#[derive(Debug, Clone, PartialEq)]
pub struct SongTags {
	pub disc_number: Option<u32>,
	pub track_number: Option<u32>,
	pub title: Option<String>,
	pub duration: Option<u32>,
	pub artist: Option<String>,
	pub album_artist: Option<String>,
	pub album: Option<String>,
	pub year: Option<i32>,
	pub has_artwork: bool,
	pub lyricist: Option<String>,
	pub composer: Option<String>,
	pub genre: Option<String>,
	pub label: Option<String>,
}

impl From<id3::Tag> for SongTags {
	fn from(tag: id3::Tag) -> Self {
		let artist = tag.artist().map(|s| s.to_string());
		let album_artist = tag.album_artist().map(|s| s.to_string());
		let album = tag.album().map(|s| s.to_string());
		let title = tag.title().map(|s| s.to_string());
		let duration = tag.duration();
		let disc_number = tag.disc();
		let track_number = tag.track();
		let year = tag
			.year()
			.map(|y| y as i32)
			.or_else(|| tag.date_released().map(|d| d.year))
			.or_else(|| tag.date_recorded().map(|d| d.year));
		let has_artwork = tag.pictures().count() > 0;
		let lyricist = tag.get_text("TEXT");
		let composer = tag.get_text("TCOM");
		let genre = tag.genre().map(|s| s.to_string());
		let label = tag.get_text("TPUB");

		SongTags {
			disc_number,
			track_number,
			title,
			duration,
			artist,
			album_artist,
			album,
			year,
			has_artwork,
			lyricist,
			composer,
			genre,
			label,
		}
	}
}

pub fn read(path: &Path) -> Option<SongTags> {
	let data = match utils::get_audio_format(path) {
		Some(AudioFormat::AIFF) => Some(read_aiff(path)),
		Some(AudioFormat::APE) => Some(read_ape(path)),
		Some(AudioFormat::FLAC) => Some(read_flac(path)),
		Some(AudioFormat::MP3) => Some(read_mp3(path)),
		Some(AudioFormat::MP4) => Some(read_mp4(path)),
		Some(AudioFormat::MPC) => Some(read_ape(path)),
		Some(AudioFormat::OGG) => Some(read_vorbis(path)),
		Some(AudioFormat::OPUS) => Some(read_opus(path)),
		Some(AudioFormat::WAVE) => Some(read_wave(path)),
		None => None,
	};
	match data {
		Some(Ok(d)) => Some(d),
		Some(Err(e)) => {
			error!("Error while reading file metadata for '{:?}': {}", path, e);
			None
		}
		None => None,
	}
}

trait FrameContent {
	/// Returns the value stored, if any, in the Frame.
	/// Say "TCOM" returns composer field.
	fn get_text(&self, key: &str) -> Option<String>;
}

impl FrameContent for id3::Tag {
	fn get_text(&self, key: &str) -> Option<String> {
		let frame = self.get(key)?;
		match frame.content() {
			id3::Content::Text(value) => Some(value.to_string()),
			_ => None,
		}
	}
}

fn read_mp3(path: &Path) -> Result<SongTags> {
	let tag = id3::Tag::read_from_path(&path).or_else(|error| {
		if let Some(tag) = error.partial_tag {
			Ok(tag)
		} else {
			Err(error)
		}
	})?;

	let duration = {
		mp3_duration::from_path(&path)
			.map(|d| d.as_secs() as u32)
			.ok()
	};

	let mut song_tags: SongTags = tag.into();
	song_tags.duration = duration; // Use duration from mp3_duration instead of from tags.
	Ok(song_tags)
}

fn read_aiff(path: &Path) -> Result<SongTags> {
	let tag = id3::Tag::read_from_aiff(&path).or_else(|error| {
		if let Some(tag) = error.partial_tag {
			Ok(tag)
		} else {
			Err(error)
		}
	})?;
	Ok(tag.into())
}

fn read_wave(path: &Path) -> Result<SongTags> {
	let tag = id3::Tag::read_from_wav(&path).or_else(|error| {
		if let Some(tag) = error.partial_tag {
			Ok(tag)
		} else {
			Err(error)
		}
	})?;
	Ok(tag.into())
}

fn read_ape_string(item: &ape::Item) -> Option<String> {
	match item.value {
		ape::ItemValue::Text(ref s) => Some(s.clone()),
		_ => None,
	}
}

fn read_ape_i32(item: &ape::Item) -> Option<i32> {
	match item.value {
		ape::ItemValue::Text(ref s) => s.parse::<i32>().ok(),
		_ => None,
	}
}

fn read_ape_x_of_y(item: &ape::Item) -> Option<u32> {
	match item.value {
		ape::ItemValue::Text(ref s) => {
			let format = Regex::new(r#"^\d+"#).unwrap();
			if let Some(m) = format.find(s) {
				s[m.start()..m.end()].parse().ok()
			} else {
				None
			}
		}
		_ => None,
	}
}

fn read_ape(path: &Path) -> Result<SongTags> {
	let tag = ape::read(path)?;
	let artist = tag.item("Artist").and_then(read_ape_string);
	let album = tag.item("Album").and_then(read_ape_string);
	let album_artist = tag.item("Album artist").and_then(read_ape_string);
	let title = tag.item("Title").and_then(read_ape_string);
	let year = tag.item("Year").and_then(read_ape_i32);
	let disc_number = tag.item("Disc").and_then(read_ape_x_of_y);
	let track_number = tag.item("Track").and_then(read_ape_x_of_y);
	let lyricist = tag.item("LYRICIST").and_then(read_ape_string);
	let composer = tag.item("COMPOSER").and_then(read_ape_string);
	let genre = tag.item("GENRE").and_then(read_ape_string);
	let label = tag.item("PUBLISHER").and_then(read_ape_string);
	Ok(SongTags {
		//
		artist,       //
		album_artist, //
		album,
		title,
		duration: None,
		disc_number,
		track_number,
		year,
		has_artwork: false,
		lyricist,
		composer,
		genre,
		label,
	})
}

fn read_vorbis(path: &Path) -> Result<SongTags> {
	let file = fs::File::open(path)?;
	let source = OggStreamReader::new(file)?;

	let mut tags = SongTags {
		artist: None,
		album_artist: None,
		album: None,
		title: None,
		duration: None,
		disc_number: None,
		track_number: None,
		year: None,
		has_artwork: false,
		lyricist: None,
		composer: None,
		genre: None,
		label: None,
	};

	for (key, value) in source.comment_hdr.comment_list {
		utils::match_ignore_case! {
			match key {
				"TITLE" => tags.title = Some(value),
				"ALBUM" => tags.album = Some(value),
				"ARTIST" => tags.artist = Some(value),
				"ALBUMARTIST" => tags.album_artist = Some(value),
				"TRACKNUMBER" => tags.track_number = value.parse::<u32>().ok(),
				"DISCNUMBER" => tags.disc_number = value.parse::<u32>().ok(),
				"DATE" => tags.year = value.parse::<i32>().ok(),
				"LYRICIST" => tags.lyricist = Some(value),
				"COMPOSER" => tags.composer = Some(value),
				"GENRE" => tags.genre = Some(value),
				"PUBLISHER" => tags.label = Some(value),
				_ => (),
			}
		}
	}

	Ok(tags)
}

fn read_opus(path: &Path) -> Result<SongTags> {
	let headers = opus_headers::parse_from_path(path)?;

	let mut tags = SongTags {
		artist: None,
		album_artist: None,
		album: None,
		title: None,
		duration: None,
		disc_number: None,
		track_number: None,
		year: None,
		has_artwork: false,
		lyricist: None,
		composer: None,
		genre: None,
		label: None,
	};

	for (key, value) in headers.comments.user_comments {
		utils::match_ignore_case! {
			match key {
				"TITLE" => tags.title = Some(value),
				"ALBUM" => tags.album = Some(value),
				"ARTIST" => tags.artist = Some(value),
				"ALBUMARTIST" => tags.album_artist = Some(value),
				"TRACKNUMBER" => tags.track_number = value.parse::<u32>().ok(),
				"DISCNUMBER" => tags.disc_number = value.parse::<u32>().ok(),
				"DATE" => tags.year = value.parse::<i32>().ok(),
				"LYRICIST" => tags.lyricist = Some(value),
				"COMPOSER" => tags.composer = Some(value),
				"GENRE" => tags.genre = Some(value),
				"PUBLISHER" => tags.label = Some(value),
				_ => (),
			}
		}
	}

	Ok(tags)
}

fn read_flac(path: &Path) -> Result<SongTags> {
	let tag = metaflac::Tag::read_from_path(path)?;
	let vorbis = tag
		.vorbis_comments()
		.ok_or_else(|| anyhow!("Missing Vorbis comments"))?;
	let disc_number = vorbis
		.get("DISCNUMBER")
		.and_then(|d| d[0].parse::<u32>().ok());
	let year = vorbis.get("DATE").and_then(|d| d[0].parse::<i32>().ok());
	let mut streaminfo = tag.get_blocks(metaflac::BlockType::StreamInfo);
	let duration = match streaminfo.next() {
		Some(&metaflac::Block::StreamInfo(ref s)) => {
			Some((s.total_samples as u32 / s.sample_rate) as u32)
		}
		_ => None,
	};
	let has_artwork = tag.pictures().count() > 0;

	Ok(SongTags {
		artist: vorbis.artist().map(|v| v[0].clone()),
		album_artist: vorbis.album_artist().map(|v| v[0].clone()),
		album: vorbis.album().map(|v| v[0].clone()),
		title: vorbis.title().map(|v| v[0].clone()),
		duration,
		disc_number,
		track_number: vorbis.track(),
		year,
		has_artwork,
		lyricist: vorbis.get("LYRICIST").map(|v| v[0].clone()),
		composer: vorbis.get("COMPOSER").map(|v| v[0].clone()),
		genre: vorbis.get("GENRE").map(|v| v[0].clone()),
		label: vorbis.get("PUBLISHER").map(|v| v[0].clone()),
	})
}

fn read_mp4(path: &Path) -> Result<SongTags> {
	let mut tag = mp4ameta::Tag::read_from_path(path)?;
	let label_ident = mp4ameta::FreeformIdent::new("com.apple.iTunes", "Label");

	Ok(SongTags {
		artist: tag.take_artist(),
		album_artist: tag.take_album_artist(),
		album: tag.take_album(),
		title: tag.take_title(),
		duration: tag.duration().map(|v| v.as_secs() as u32),
		disc_number: tag.disc_number().map(|d| d as u32),
		track_number: tag.track_number().map(|d| d as u32),
		year: tag.year().and_then(|v| v.parse::<i32>().ok()),
		has_artwork: tag.artwork().is_some(),
		lyricist: tag.take_lyricist(),
		composer: tag.take_composer(),
		genre: tag.take_genre(),
		label: tag.take_string(&label_ident).next(),
	})
}

#[test]
fn reads_file_metadata() {
	let sample_tags = SongTags {
		disc_number: Some(3),
		track_number: Some(1),
		title: Some("TEST TITLE".into()),
		artist: Some("TEST ARTIST".into()),
		album_artist: Some("TEST ALBUM ARTIST".into()),
		album: Some("TEST ALBUM".into()),
		duration: None,
		year: Some(2016),
		has_artwork: false,
		lyricist: Some("TEST LYRICIST".into()),
		composer: Some("TEST COMPOSER".into()),
		genre: Some("TEST GENRE".into()),
		label: Some("TEST LABEL".into()),
	};
	let flac_sample_tag = SongTags {
		duration: Some(0),
		..sample_tags.clone()
	};
	let mp3_sample_tag = SongTags {
		duration: Some(0),
		..sample_tags.clone()
	};
	let m4a_sample_tag = SongTags {
		duration: Some(0),
		..sample_tags.clone()
	};
	assert_eq!(
		read(Path::new("test-data/formats/sample.aif")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.mp3")).unwrap(),
		mp3_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.ogg")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.flac")).unwrap(),
		flac_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.m4a")).unwrap(),
		m4a_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.opus")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.ape")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.wav")).unwrap(),
		sample_tags
	);
}

#[test]
fn reads_embedded_artwork() {
	assert!(
		read(Path::new("test-data/artwork/sample.aif"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.mp3"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.flac"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.m4a"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.wav"))
			.unwrap()
			.has_artwork
	);
}
