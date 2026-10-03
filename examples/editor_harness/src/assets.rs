//! Bounded synchronous fixture asset host; not a production asset store.
//!
//! Append-only sidecar files deliberately outlive document Undo/Redo and
//! saved snapshots. Reclamation requires knowledge of all documents/history.
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
};
use xiaomu_core::document::{ImageAttrs, ImageSource};
use xiaomu_runtime::assets::{
    AssetError, AssetFormat, AssetRef, AssetService, AssetSink, ResolvedAsset,
};

const MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct FixtureAssets {
    directory: PathBuf,
}

impl FixtureAssets {
    pub fn for_document(path: &Path) -> Self {
        let mut name = path.as_os_str().to_owned();
        name.push(".assets");
        Self {
            directory: PathBuf::from(name),
        }
    }

    fn read(&self, reference: AssetRef) -> Result<ResolvedAsset, AssetError> {
        let name = reference.value();
        let (stem, extension) = name.rsplit_once('.').ok_or(AssetError::InvalidRef)?;
        let id = stem.strip_prefix("image-").ok_or(AssetError::InvalidRef)?;
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') {
            return Err(AssetError::InvalidRef);
        }
        let format = match extension {
            "png" => AssetFormat::Png,
            "jpg" => AssetFormat::Jpeg,
            _ => return Err(AssetError::InvalidRef),
        };
        let path = self.directory.join(name);
        let metadata = std::fs::metadata(&path).map_err(io_error)?;
        if metadata.len() > MAX_BYTES as u64 {
            return Err(AssetError::InvalidImage);
        }
        let bytes = std::fs::read(path).map_err(io_error)?;
        validate(format, &bytes)?;
        Ok(ResolvedAsset::new(reference, 1, format, bytes))
    }
}

fn validate(format: AssetFormat, bytes: &[u8]) -> Result<(u32, u32), AssetError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(AssetError::InvalidImage);
    }
    let encoding = match format {
        AssetFormat::Png => image::ImageFormat::Png,
        AssetFormat::Jpeg => image::ImageFormat::Jpeg,
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), encoding);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| AssetError::InvalidImage)?;
    Ok((decoded.width(), decoded.height()))
}

fn io_error(error: std::io::Error) -> AssetError {
    match error.kind() {
        std::io::ErrorKind::NotFound => AssetError::NotFound,
        std::io::ErrorKind::PermissionDenied => AssetError::PermissionDenied,
        _ => AssetError::Unavailable,
    }
}

impl AssetService for FixtureAssets {
    fn resolve(&self, asset_ref: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(self.read(asset_ref));
    }

    fn import_image(&self, format: AssetFormat, bytes: &[u8]) -> Result<ImageAttrs, AssetError> {
        let (width, height) = validate(format, bytes)?;
        std::fs::create_dir_all(&self.directory).map_err(io_error)?;
        let extension = match format {
            AssetFormat::Png => "png",
            AssetFormat::Jpeg => "jpg",
        };
        let name = format!("image-{}.{}", crate::atomic_file::unique_id(), extension);
        crate::atomic_file::write(&self.directory.join(&name), bytes).map_err(io_error)?;
        ImageAttrs::new(
            ImageSource::AssetRef(name),
            "Pasted image".into(),
            None,
            Some(width),
            Some(height),
        )
        .map_err(|_| AssetError::InvalidImage)
    }
}

#[cfg(test)]
mod tests;
