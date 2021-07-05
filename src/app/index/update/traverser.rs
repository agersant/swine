use crossbeam_channel::{self, Receiver, Sender};
use log::{error, info};
use std::cmp::min;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::app::index::metadata::{self, SongTags};

#[derive(Debug)]
pub struct Song {
	pub path: PathBuf,
	pub metadata: SongTags,
}

#[derive(Debug)]
pub struct Directory {
	pub parent: Option<PathBuf>,
	pub path: PathBuf,
	pub songs: Vec<Song>,
	pub other_files: Vec<PathBuf>,
	pub created: i32,
}

pub struct Traverser {
	directory_sender: Sender<Directory>,
}

#[derive(Debug)]
struct WorkItem {
	parent: Option<PathBuf>,
	path: PathBuf,
}

impl Traverser {
	pub fn new(directory_sender: Sender<Directory>) -> Self {
		Self { directory_sender }
	}

	pub fn traverse(&self, roots: Vec<PathBuf>) {
		let num_pending_work_items = Arc::new(AtomicUsize::new(roots.len()));
		let (work_item_sender, work_item_receiver) = crossbeam_channel::unbounded();

		let key = "POLARIS_NUM_TRAVERSER_THREADS";
		let num_threads = std::env::var_os(key)
			.map(|v| v.to_string_lossy().to_string())
			.and_then(|v| usize::from_str(&v).ok())
			.unwrap_or_else(|| min(num_cpus::get(), 4));
		info!("Browsing collection using {} threads", num_threads);

		let mut threads = Vec::new();
		for _ in 0..num_threads {
			let work_item_sender = work_item_sender.clone();
			let work_item_receiver = work_item_receiver.clone();
			let directory_sender = self.directory_sender.clone();
			let num_pending_work_items = num_pending_work_items.clone();
			threads.push(thread::spawn(move || {
				let worker = Worker {
					work_item_sender,
					work_item_receiver,
					directory_sender,
					num_pending_work_items,
				};
				worker.run();
			}));
		}

		for root in roots {
			let work_item = WorkItem {
				parent: None,
				path: root,
			};
			if let Err(e) = work_item_sender.send(work_item) {
				error!("Error initializing traverser: {:#?}", e);
			}
		}

		for thread in threads {
			if let Err(e) = thread.join() {
				error!("Error joining on traverser worker thread: {:#?}", e);
			}
		}
	}
}

struct Worker {
	work_item_sender: Sender<WorkItem>,
	work_item_receiver: Receiver<WorkItem>,
	directory_sender: Sender<Directory>,
	num_pending_work_items: Arc<AtomicUsize>,
}

impl Worker {
	fn run(&self) {
		while let Some(work_item) = self.find_work_item() {
			self.process_work_item(work_item);
			self.on_item_processed();
		}
	}

	fn find_work_item(&self) -> Option<WorkItem> {
		loop {
			if self.is_all_work_done() {
				return None;
			}
			if let Ok(w) = self
				.work_item_receiver
				.recv_timeout(Duration::from_millis(100))
			{
				return Some(w);
			}
		}
	}

	fn is_all_work_done(&self) -> bool {
		self.num_pending_work_items.load(Ordering::SeqCst) == 0
	}

	fn queue_work(&self, work_item: WorkItem) {
		self.num_pending_work_items.fetch_add(1, Ordering::SeqCst);
		self.work_item_sender.send(work_item).unwrap();
	}

	fn on_item_processed(&self) {
		self.num_pending_work_items.fetch_sub(1, Ordering::SeqCst);
	}

	fn emit_directory(&self, directory: Directory) {
		self.directory_sender.send(directory).unwrap();
	}

	pub fn process_work_item(&self, work_item: WorkItem) {
		let read_dir = match fs::read_dir(&work_item.path) {
			Ok(read_dir) => read_dir,
			Err(e) => {
				error!(
					"Directory read error for `{}`: {}",
					work_item.path.display(),
					e
				);
				return;
			}
		};

		let mut sub_directories = Vec::new();
		let mut songs = Vec::new();
		let mut other_files = Vec::new();

		for entry in read_dir {
			let path = match entry {
				Ok(ref f) => f.path(),
				Err(e) => {
					error!(
						"File read error within `{}`: {}",
						work_item.path.display(),
						e
					);
					break;
				}
			};

			if path.is_dir() {
				sub_directories.push(path);
			} else if let Some(metadata) = metadata::read(&path) {
				songs.push(Song { path, metadata });
			} else {
				other_files.push(path);
			}
		}

		let created = Self::get_date_created(&work_item.path).unwrap_or_default();

		self.emit_directory(Directory {
			path: work_item.path.to_owned(),
			parent: work_item.parent,
			songs,
			other_files,
			created,
		});

		for sub_directory in sub_directories.into_iter() {
			self.queue_work(WorkItem {
				parent: Some(work_item.path.clone()),
				path: sub_directory,
			});
		}
	}

	fn get_date_created(path: &Path) -> Option<i32> {
		if let Ok(t) = fs::metadata(path).and_then(|m| m.created().or_else(|_| m.modified())) {
			t.duration_since(std::time::UNIX_EPOCH)
				.map(|d| d.as_secs() as i32)
				.ok()
		} else {
			None
		}
	}
}
