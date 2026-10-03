//! Same-directory replacement prevents a failed write from truncating a snapshot.
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn unique_id() -> String {
    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_with(
        path,
        |file| file.write_all(bytes),
        |from, to| std::fs::rename(from, to),
    )
}

fn write_with(
    path: &Path,
    write_bytes: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temporary = path.with_file_name(format!(".xiaomu-{}.tmp", unique_id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        write_bytes(&mut file)?;
        file.sync_all()?;
        publish(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_destination_survives_partial_write_and_publish_failure() {
        let directory = std::env::temp_dir().join(format!("xiaomu-atomic-{}", unique_id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("snapshot");
        write(&path, b"old").unwrap();
        let failure = || std::io::Error::other("injected failure");
        assert!(
            write_with(
                &path,
                |file| {
                    file.write_all(b"partial")?;
                    Err(failure())
                },
                |from, to| std::fs::rename(from, to)
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert!(write_with(&path, |file| file.write_all(b"new"), |_, _| Err(failure())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        write(&path, b"replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
