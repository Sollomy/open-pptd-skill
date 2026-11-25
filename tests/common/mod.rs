#![allow(dead_code)]

use std::fs;
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// Unique temporary directory that cleans itself up on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let seq = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "open-pptd-test-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Write a minimal deck into a fresh RAII temp directory and return `(dir, entry_path)`.
pub fn write_deck(tag: &str, deck_yaml: &str, pages: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new(tag);
    fs::create_dir_all(dir.join("pages")).unwrap();
    let entry = dir.join("deck.pptd");
    fs::write(&entry, deck_yaml).unwrap();
    for (name, body) in pages {
        let target = dir.join("pages").join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, body).unwrap();
    }
    (dir, entry)
}

pub fn read_zip_part(path: &Path, name: &str) -> String {
    let file = fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut data = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut data)
        .unwrap();
    data
}

pub fn read_zip_bytes(path: &Path, name: &str) -> Vec<u8> {
    let file = fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut data = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut data).unwrap();
    data
}
