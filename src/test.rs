use std::path::PathBuf;

#[macro_export]
macro_rules! test_name {
	() => {{
		let file_name = file!();
		let file_name = file_name.replace("/", "-");
		let file_name = file_name.replace("\\", "-");
		format!("{}-line-{}", file_name, line!())
	}};
}

pub fn prepare_test_directory<T: AsRef<str>>(test_name: T) -> PathBuf {
	let output_dir: PathBuf = [".", "test-output", test_name.as_ref()].iter().collect();
	if output_dir.is_dir() {
		std::fs::remove_dir_all(&output_dir).unwrap();
	}
	std::fs::create_dir_all(&output_dir).unwrap();
	return output_dir;
}
