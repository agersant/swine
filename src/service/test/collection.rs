use http::StatusCode;
use std::path::{Path, PathBuf};

use crate::app::index;
use crate::service::test::{add_trailing_slash, constants::*, protocol, ServiceType, TestService};
use crate::test_name;

const TEST_ALL_SONGS_COUNT: usize = 13;

#[test]
fn browse_requires_auth() {
	let mut service = ServiceType::new(&test_name!());
	let request = protocol::browse(&PathBuf::new());
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn browse_root() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::browse(&PathBuf::new());
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 1);
}

#[test]
fn browse_directory() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let path: PathBuf = [TEST_MOUNT_NAME, "Khemmis", "Hunted"].iter().collect();
	let request = protocol::browse(&path);
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 5);
}

#[test]
fn browse_bad_directory() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login();

	let path: PathBuf = ["not_my_collection"].iter().collect();
	let request = protocol::browse(&path);
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn flatten_requires_auth() {
	let mut service = ServiceType::new(&test_name!());
	let request = protocol::flatten(&PathBuf::new());
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn flatten_root() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::flatten(&PathBuf::new());
	let response = service.fetch_json::<_, Vec<index::Song>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), TEST_ALL_SONGS_COUNT);
}

#[test]
fn flatten_directory() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::flatten(Path::new(TEST_MOUNT_NAME));
	let response = service.fetch_json::<_, Vec<index::Song>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), TEST_ALL_SONGS_COUNT);
}

#[test]
fn flatten_bad_directory() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login();

	let path: PathBuf = ["not_my_collection"].iter().collect();
	let request = protocol::flatten(&path);
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn random_requires_auth() {
	let mut service = ServiceType::new(&test_name!());
	let request = protocol::random();
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn random_golden_path() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::random();
	let response = service.fetch_json::<_, Vec<index::Directory>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 3);
}

#[test]
fn random_with_trailing_slash() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let mut request = protocol::random();
	add_trailing_slash(&mut request);
	let response = service.fetch_json::<_, Vec<index::Directory>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 3);
}

#[test]
fn recent_requires_auth() {
	let mut service = ServiceType::new(&test_name!());
	let request = protocol::recent();
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn recent_golden_path() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::recent();
	let response = service.fetch_json::<_, Vec<index::Directory>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 3);
}

#[test]
fn recent_with_trailing_slash() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let mut request = protocol::recent();
	add_trailing_slash(&mut request);
	let response = service.fetch_json::<_, Vec<index::Directory>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
	let entries = response.body();
	assert_eq!(entries.len(), 3);
}

#[test]
fn search_requires_auth() {
	let mut service = ServiceType::new(&test_name!());
	let request = protocol::search("");
	let response = service.fetch(&request);
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn search_without_query() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login();

	let request = protocol::search("");
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn search_with_query() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::search("door");
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	let results = response.body();
	assert_eq!(results.len(), 1);
	match results[0] {
		index::CollectionFile::Song(ref s) => {
			assert_eq!(s.title, Some("Beyond The Door".into()))
		}
		_ => panic!(),
	}
}

#[test]
fn search_extended_tags() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::search("Electron");
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	let results = response.body();
	assert_eq!(results.len(), 8);
	match results[0] {
		index::CollectionFile::Song(ref s) => {
			assert_eq!(s.title, Some("ピクニック (Picnic) (Remix)".into()))
		}
		_ => panic!(),
	}
}

#[test]
fn search_with_query_one_field() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::search("album:Remixe");
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	let results = response.body();
	assert_eq!(results.len(), 1);
	match results[0] {
		index::CollectionFile::Song(ref s) => {
			assert_eq!(s.title, Some("ピクニック (Picnic) (Remix)".into()))
		}
		_ => panic!(),
	}
}

#[test]
fn search_with_query_multiple_fields() {
	let mut service = ServiceType::new(&test_name!());
	service.complete_initial_setup();
	service.login_admin();
	service.index();
	service.login();

	let request = protocol::search("title:game genre:'Electronic\u{0}Chiptune'");
	let response = service.fetch_json::<_, Vec<index::CollectionFile>>(&request);
	let results = response.body();
	assert_eq!(results.len(), 1);
	match results[0] {
		index::CollectionFile::Song(ref s) => {
			assert_eq!(
				s.title,
				Some("疲れた顔(ゲームは終わらない) (Tired Face (The Game Never Ends))".into())
			)
		}
		_ => panic!(),
	}
}
