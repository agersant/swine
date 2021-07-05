use std::path::Path;

#[macro_export]
macro_rules! match_ignore_case {
    (match $v:ident {
        $( $lit:literal => $res:expr, )*
        _ => $catch_all:expr $(,)?
    }) => {{
        $( if $lit.eq_ignore_ascii_case(&$v) { $res } else )*
        { $catch_all }
    }};
}
pub use crate::match_ignore_case;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq)]
pub enum AudioFormat {
	AIFF,
	APE,
	FLAC,
	MP3,
	MP4,
	MPC,
	OGG,
	OPUS,
	WAVE,
}

pub fn get_audio_format(path: &Path) -> Option<AudioFormat> {
	let extension = match path.extension() {
		Some(e) => e,
		_ => return None,
	};
	let extension = match extension.to_str() {
		Some(e) => e,
		_ => return None,
	};
	match extension.to_lowercase().as_str() {
		"aif" => Some(AudioFormat::AIFF),
		"aiff" => Some(AudioFormat::AIFF),
		"ape" => Some(AudioFormat::APE),
		"flac" => Some(AudioFormat::FLAC),
		"mp3" => Some(AudioFormat::MP3),
		"m4a" => Some(AudioFormat::MP4),
		"mpc" => Some(AudioFormat::MPC),
		"ogg" => Some(AudioFormat::OGG),
		"opus" => Some(AudioFormat::OPUS),
		"wav" => Some(AudioFormat::WAVE),
		_ => None,
	}
}

#[test]
fn can_guess_audio_format() {
	assert_eq!(get_audio_format(Path::new("animals/🐷/my🐖file.jpg")), None);
	assert_eq!(
		get_audio_format(Path::new("animals/🐷/my🐖file.aif")),
		Some(AudioFormat::AIFF)
	);
	assert_eq!(
		get_audio_format(Path::new("animals/🐷/my🐖file.aiff")),
		Some(AudioFormat::AIFF)
	);
	assert_eq!(
		get_audio_format(Path::new("animals/🐷/my🐖file.flac")),
		Some(AudioFormat::FLAC)
	);
	assert_eq!(
		get_audio_format(Path::new("animals/🐷/my🐖file.wav")),
		Some(AudioFormat::WAVE)
	);
}
