use diesel::prelude::*;
use std::default::Default;
use std::path::{Path, PathBuf};

use super::*;
use crate::app::test;
use crate::db::{directories, songs};
use crate::test_name;

const TEST_MOUNT_NAME: &str = "root";
const TEST_ALL_SONGS_COUNT: usize = 13;
const TEST_DIRECTORIES_COUNT: usize = 6;

#[test]
fn update_adds_new_content() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();

	ctx.index.update().unwrap();
	ctx.index.update().unwrap(); // Validates that subsequent updates don't run into conflicts

	let connection = ctx.db.connect().unwrap();
	let all_directories: Vec<Directory> = directories::table.load(&connection).unwrap();
	let all_songs: Vec<Song> = songs::table.load(&connection).unwrap();
	assert_eq!(all_directories.len(), TEST_DIRECTORIES_COUNT);
	assert_eq!(all_songs.len(), TEST_ALL_SONGS_COUNT);
}

#[test]
fn update_removes_missing_content() {
	let builder = test::ContextBuilder::new(test_name!());

	let original_collection_dir: PathBuf = ["test-data", "small-collection"].iter().collect();
	let test_collection_dir: PathBuf = builder.test_directory.join("small-collection");

	let copy_options = fs_extra::dir::CopyOptions::new();
	fs_extra::dir::copy(
		&original_collection_dir,
		&builder.test_directory,
		&copy_options,
	)
	.unwrap();

	let ctx = builder
		.mount(TEST_MOUNT_NAME, test_collection_dir.to_str().unwrap())
		.build();

	ctx.index.update().unwrap();

	{
		let connection = ctx.db.connect().unwrap();
		let all_directories: Vec<Directory> = directories::table.load(&connection).unwrap();
		let all_songs: Vec<Song> = songs::table.load(&connection).unwrap();
		assert_eq!(all_directories.len(), TEST_DIRECTORIES_COUNT);
		assert_eq!(all_songs.len(), TEST_ALL_SONGS_COUNT);
	}

	let khemmis_directory = test_collection_dir.join("Khemmis");
	std::fs::remove_dir_all(&khemmis_directory).unwrap();
	ctx.index.update().unwrap();
	{
		let connection = ctx.db.connect().unwrap();
		let all_directories: Vec<Directory> = directories::table.load(&connection).unwrap();
		let all_songs: Vec<Song> = songs::table.load(&connection).unwrap();
		assert_eq!(all_directories.len(), 4);
		assert_eq!(all_songs.len(), 8);
	}
}

#[test]
fn can_browse_top_level() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();

	let root_path = Path::new(TEST_MOUNT_NAME);
	let files = ctx.index.browse(Path::new("")).unwrap();
	assert_eq!(files.len(), 1);
	match files[0] {
		CollectionFile::Directory(ref d) => assert_eq!(d.path, root_path.to_str().unwrap()),
		_ => panic!("Expected directory"),
	}
}

#[test]
fn can_browse_directory() {
	let khemmis_path: PathBuf = [TEST_MOUNT_NAME, "Khemmis"].iter().collect();
	let tobokegao_path: PathBuf = [TEST_MOUNT_NAME, "Tobokegao"].iter().collect();

	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();

	let files = ctx.index.browse(Path::new(TEST_MOUNT_NAME)).unwrap();

	assert_eq!(files.len(), 2);
	if let (CollectionFile::Directory(ref d1), CollectionFile::Directory(ref d2)) =
		(&files[0], &files[1])
	{
		if d1.path == khemmis_path.to_str().unwrap() {
			assert_eq!(d2.path, tobokegao_path.to_str().unwrap());
		} else {
			assert_eq!(d2.path, khemmis_path.to_str().unwrap());
		}
	}
}

#[test]
fn can_flatten_root() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();
	let songs = ctx.index.flatten(Path::new(TEST_MOUNT_NAME)).unwrap();
	assert_eq!(songs.len(), TEST_ALL_SONGS_COUNT);
	assert_eq!(songs[0].title, Some("Above The Water".to_owned()));
}

#[test]
fn can_flatten_directory() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();
	let path: PathBuf = [TEST_MOUNT_NAME, "Tobokegao"].iter().collect();
	let songs = ctx.index.flatten(&path).unwrap();
	assert_eq!(songs.len(), 8);
}

#[test]
fn can_flatten_directory_with_shared_prefix() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();
	let path: PathBuf = [TEST_MOUNT_NAME, "Tobokegao", "Picnic"].iter().collect(); // Prefix of '(Picnic Remixes)'
	let songs = ctx.index.flatten(&path).unwrap();
	assert_eq!(songs.len(), 7);
}

#[test]
fn can_get_random_albums() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();
	let albums = ctx.index.get_random_albums(1).unwrap();
	assert_eq!(albums.len(), 1);
}

#[test]
fn can_get_recent_albums() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();
	ctx.index.update().unwrap();
	let albums = ctx.index.get_recent_albums(2).unwrap();
	assert_eq!(albums.len(), 2);
	assert!(albums[0].date_added >= albums[1].date_added);
}

#[test]
fn can_get_a_song() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();

	ctx.index.update().unwrap();

	let picnic_virtual_dir: PathBuf = [TEST_MOUNT_NAME, "Tobokegao", "Picnic"].iter().collect();
	let song_virtual_path = picnic_virtual_dir.join("05 - シャーベット (Sherbet).mp3");
	let artwork_virtual_path = picnic_virtual_dir.join("Folder.png");

	let song = ctx.index.get_song(&song_virtual_path).unwrap();
	assert_eq!(song.path, song_virtual_path.to_string_lossy().as_ref());
	assert_eq!(song.track_number, Some(5));
	assert_eq!(song.disc_number, None);
	assert_eq!(song.title, Some("シャーベット (Sherbet)".to_owned()));
	assert_eq!(song.artist, Some("Tobokegao".to_owned()));
	assert_eq!(song.album_artist, None);
	assert_eq!(song.album, Some("Picnic".to_owned()));
	assert_eq!(song.year, Some(2016));
	assert_eq!(
		song.artwork,
		Some(artwork_virtual_path.to_string_lossy().into_owned())
	);
}

#[test]
fn indexes_embedded_artwork() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();

	ctx.index.update().unwrap();

	let picnic_virtual_dir: PathBuf = [TEST_MOUNT_NAME, "Tobokegao", "Picnic"].iter().collect();
	let song_virtual_path = picnic_virtual_dir.join("07 - なぜ (Why).mp3");

	let song = ctx.index.get_song(&song_virtual_path).unwrap();
	assert_eq!(
		song.artwork,
		Some(song_virtual_path.to_string_lossy().into_owned())
	);
}

#[test]
fn album_art_pattern_is_case_insensitive() {
	let ctx = test::ContextBuilder::new(test_name!())
		.mount(TEST_MOUNT_NAME, "test-data/small-collection")
		.build();

	let patterns = vec!["folder", "FOLDER"]
		.iter()
		.map(|s| s.to_string())
		.collect::<Vec<_>>();

	for pattern in patterns.into_iter() {
		ctx.settings_manager
			.amend(&settings::NewSettings {
				album_art_pattern: Some(pattern),
				..Default::default()
			})
			.unwrap();
		ctx.index.update().unwrap();

		let hunted_virtual_dir: PathBuf = [TEST_MOUNT_NAME, "Khemmis", "Hunted"].iter().collect();
		let artwork_virtual_path = hunted_virtual_dir.join("Folder.jpg");
		let song = &ctx.index.flatten(&hunted_virtual_dir).unwrap()[0];
		assert_eq!(
			song.artwork,
			Some(artwork_virtual_path.to_string_lossy().into_owned())
		);
	}
}

#[test]
fn query_string_empty_string() {
	let query = QueryFields {
		general_query: Some("".to_string()),
		..Default::default()
	};
	assert_eq!(query, parse_query(""));
}

#[test]
fn query_string_generic_query() {
	let query = QueryFields {
		general_query: Some("generic query".to_string()),
		..Default::default()
	};
	assert_eq!(query, parse_query("generic query"));
}

#[test]
fn query_string_tokern_empty_string() {
	let query = QueryFields {
		general_query: Some("".to_string()),
		..Default::default()
	};
	assert_eq!(query, parse_query("artist:"));
}
#[test]
fn query_string_token_at_start() {
	let query = QueryFields {
		general_query: Some("generic query".to_string()),
		composer: Some("%test_composer%".to_string()),
		..Default::default()
	};
	assert_eq!(query, parse_query("composer:TEST_COMPOSER generic query"));
}

#[test]
fn query_string_token_at_end() {
	let query = QueryFields {
		general_query: Some("generic query".to_string()),
		composer: Some("%est composer%".to_string()),
		..Default::default()
	};
	assert_eq!(
		query,
		parse_query("generic query composer:\"est COMPOSER\"")
	);
}

#[test]
fn query_string_token_in_the_middle() {
	let query = QueryFields {
		general_query: Some("generic query generic2 query2".to_string()),
		composer: Some("%test composer%".to_string()),
		..Default::default()
	};
	assert_eq!(
		query,
		parse_query("generic query composer:'TEST COMPOSER'  generic2  query2 ")
	);
}

#[test]
// Repeated tokens are considered malformed query string.
fn query_string_repeated_token_should_not_be_parsed() {
	let query = QueryFields {
		general_query: Some(
			"artist:\" singer1 \" generic query artist:'singer2 ' generic2 query2".to_string(),
		),
		..Default::default()
	};
	assert_eq!(
		query,
		parse_query(
			"  artist:\"  SinGer1 \"  generic \t query \n artist:'SINGER2 '  generic2  query2  "
		)
	);
}

#[test]
fn query_string_multiple_space_trim() {
	let query = QueryFields {
		general_query: Some("generic query generic2 query2".to_string()),
		composer: Some("%first1 last1%".to_string()),
		artist: Some("%first2 last2%".to_string()),
		..Default::default()
	};
	assert_eq!(
		query,
		parse_query(
			"  artist:\"  fIrst2  LasT2 \"  generic \t query \n composer:'FiRST1 LAST1  '  \
		   generic2  query2  "
		)
	);
}

#[test]
fn query_string_all_fields() {
	let query = QueryFields {
		general_query: Some("generic query generic2 query2".to_string()),
		composer: Some("%first1 last1%".to_string()),
		artist: Some("%first2 last2%".to_string()),
		lyricist: Some("%lyricist1%".to_string()),
		album: Some("%album1%".to_string()),
		album_artist: Some("%album_artist1%".to_string()),
		title: Some("%part of title%".to_string()),
		genre: Some("%genre1%".to_string()),
	};
	assert_eq!(
		query,
		parse_query(
			"  artist:\"  fIrst2  LasT2 \"  generic \t query \n composer:'FiRST1 LAST1  '  \
		   generic2  query2  lyricist:lyricist1 title:'Part OF TITLE' album:'album1' album_artist:\
		   'album_artist1' genre:genre1"
		)
	);
}
