use anyhow::{Context, Result, ensure};
use localsend::model::transfer::{FileDto, FileMetadata};
use localsend::util::filename::{self, Rules};
use rustsend_protocol::transfer::FileItem;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct SelectedFile {
    pub dto: FileDto,
    pub path: PathBuf,
}

impl SelectedFile {
    pub fn view(&self) -> FileItem {
        view(&self.dto, true)
    }
}

pub fn view(file: &FileDto, selected: bool) -> FileItem {
    FileItem {
        id: file.id.clone(),
        name: file.file_name.clone(),
        size: format_bytes(file.size),
        selected,
    }
}

pub fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut i = 0;
    while value >= 1000.0 && i < units.len() - 1 {
        value /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[i])
    }
}

/// Preserve folder structure, skip symlinks and fail atomically on unreadable entries.
pub fn collect(paths: Vec<PathBuf>) -> Result<Vec<SelectedFile>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let path = path
            .canonicalize()
            .with_context(|| format!("Cannot read {}", path.display()))?;
        if path.is_file() {
            let name = path
                .file_name()
                .context("File has no name")?
                .to_string_lossy()
                .into_owned();
            add(&mut result, &mut seen, path, name)?;
        } else {
            ensure!(path.is_dir(), "Not a file or folder: {}", path.display());
            let root = path
                .file_name()
                .context("Choose a named folder")?
                .to_string_lossy();
            for entry in walkdir::WalkDir::new(&path)
                .min_depth(1)
                .sort_by_file_name()
            {
                let entry = entry?;
                if entry.file_type().is_file() {
                    let relative = entry.path().strip_prefix(&path)?;
                    let name = format!("{root}/{}", relative.to_string_lossy().replace('\\', "/"));
                    add(&mut result, &mut seen, entry.path().to_path_buf(), name)?;
                }
                ensure!(
                    result.len() <= 10000,
                    "Select at most 10,000 files in one batch"
                );
            }
        }
    }
    ensure!(!result.is_empty(), "No regular files in the selection");
    Ok(result)
}

fn add(
    out: &mut Vec<SelectedFile>,
    seen: &mut HashSet<PathBuf>,
    path: PathBuf,
    name: String,
) -> Result<()> {
    if !seen.insert(path.clone()) {
        return Ok(());
    }
    let file = std::fs::File::open(&path)?;
    let meta = file.metadata()?;
    out.push(SelectedFile {
        dto: FileDto {
            id: Uuid::new_v4().to_string(),
            file_name: name,
            size: meta.len(),
            file_type: mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string(),
            sha256: None,
            preview: None,
            metadata: FileMetadata::from_fs_metadata(&meta),
        },
        path,
    });
    Ok(())
}

/// Reject traversal before sanitizing. Backslashes are path separators on every host.
pub fn relative_name(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.starts_with(['/', '\\']),
        "Invalid relative file name"
    );
    let normalized = name.replace('\\', "/");
    let mut path = PathBuf::new();
    for (i, component) in normalized.split('/').enumerate() {
        ensure!(
            !component.is_empty() && component != "." && component != "..",
            "Unsafe file name: {name}"
        );
        ensure!(
            !(i == 0 && component.contains(':')),
            "Absolute path is not allowed"
        );
        path.push(filename::sanitize(component, Rules::Universal));
    }
    ensure!(
        path.components().count() <= 32,
        "Folder nesting is too deep"
    );
    Ok(path)
}

/// Each transfer is received into its own private temporary directory.
/// Persist only fully verified files; no partial file replaces an existing user file.
pub fn persist_received(temp: &Path, destination: &Path, name: &str) -> Result<PathBuf> {
    let relative = relative_name(name)?;
    ensure!(
        std::fs::symlink_metadata(temp)?.is_file(),
        "Received content must be a regular file"
    );
    let base = destination.canonicalize()?;
    let mut parent = base.clone();
    if let Some(parts) = relative.parent() {
        for component in parts.components() {
            parent.push(component);
            match std::fs::symlink_metadata(&parent) {
                Ok(meta) => ensure!(
                    meta.is_dir() && !meta.file_type().is_symlink(),
                    "Receive folder contains a symlink or a file"
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&parent)?,
                Err(e) => return Err(e.into()),
            }
        }
    }
    ensure!(
        parent.canonicalize()?.starts_with(&base),
        "Receive path escapes the destination"
    );
    let name = relative.file_name().context("Missing file name")?;
    let stem = Path::new(name)
        .file_stem()
        .unwrap_or(name)
        .to_string_lossy();
    let extension = Path::new(name)
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for n in 0..10000 {
        let candidate = parent.join(if n == 0 {
            name.to_string_lossy().into_owned()
        } else {
            format!("{stem} ({n}){extension}")
        });
        // Atomic no-clobber publication on the same destination filesystem.
        match publish_no_clobber(temp, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                // The UI displays the outer message; keep the OS reason visible.
                let context = format!("Cannot publish received file: {e}");
                return Err(e).context(context);
            }
        }
    }
    anyhow::bail!("Too many files with the same name")
}

/// Publish verified content atomically without replacing any existing entry.
/// Android app storage can reject hard links. `rustix` invokes renameat2
/// through the syscall on Android, so this also works before libc exposes
/// renameat2 at API 30. The staging and receive folders share app storage.
#[cfg(target_os = "android")]
fn publish_no_clobber(source: &Path, destination: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(not(target_os = "android"))]
fn publish_no_clobber(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::hard_link(source, destination)?;
    std::fs::remove_file(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn publication_failure_preserves_existing_content_and_staging() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("staged");
        let destination = root.path().join("existing");
        std::fs::write(&source, b"new verified content").unwrap();
        std::fs::write(&destination, b"original content").unwrap();

        let error = publish_no_clobber(&source, &destination).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&source).unwrap(), b"new verified content");
        assert_eq!(std::fs::read(&destination).unwrap(), b"original content");

        assert!(publish_no_clobber(&source, &root.path().join("missing/file")).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"new verified content");
        assert!(!root.path().join("missing/file").exists());
    }

    #[test]
    fn simultaneous_receives_never_overwrite_or_expose_partial_content() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("received");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("测试.png"), b"original").unwrap();
        // An existing directory also occupies a name; it must remain untouched.
        std::fs::create_dir(destination.join("测试 (1).png")).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let mut receivers = Vec::new();
        for index in 0..8u8 {
            let source = root.path().join(format!("staged-{index}"));
            let contents = vec![index; 128 * 1024 + usize::from(index)];
            std::fs::write(&source, &contents).unwrap();
            let destination = destination.clone();
            let barrier = barrier.clone();
            receivers.push(std::thread::spawn(move || {
                barrier.wait();
                let published = persist_received(&source, &destination, "测试.png").unwrap();
                assert!(!source.exists());
                assert_eq!(std::fs::read(&published).unwrap(), contents);
                published
            }));
        }
        let published: HashSet<_> = receivers.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(published.len(), 8);
        assert_eq!(
            std::fs::read(destination.join("测试.png")).unwrap(),
            b"original"
        );
        assert!(destination.join("测试 (1).png").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn publication_does_not_follow_symlink_collisions_or_parents() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("received");
        std::fs::create_dir(&destination).unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let original = outside.join("original");
        std::fs::write(&original, b"keep").unwrap();
        std::os::unix::fs::symlink(&original, destination.join("data.txt")).unwrap();
        let source = root.path().join("staged");
        std::fs::write(&source, b"verified").unwrap();
        let published = persist_received(&source, &destination, "data.txt").unwrap();
        assert_eq!(published.file_name().unwrap(), "data (1).txt");
        assert_eq!(std::fs::read(&original).unwrap(), b"keep");
        assert!(destination.join("data.txt").is_symlink());

        std::os::unix::fs::symlink(&outside, destination.join("folder")).unwrap();
        std::fs::write(&source, b"verified").unwrap();
        assert!(persist_received(&source, &destination, "folder/escape.txt").is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"verified");
        assert!(!outside.join("escape.txt").exists());
    }
}
