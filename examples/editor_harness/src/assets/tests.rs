use super::*;
use crate::store::{FixtureStore, canonical_semantics_equal, caret_at_first_block, demo_fixture};
use std::cell::RefCell;
use xiaomu_runtime::{
    persistence::DocumentPersistence,
    session::{DocumentSelection, DocumentSession, EditIntent},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("xiaomu-images-{}", crate::atomic_file::unique_id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn document(&self) -> PathBuf {
        self.0.join("document.txt")
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        8,
        8,
        image::Rgba([255, 0, 0, 255]),
    ));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
fn reference(image: &ImageAttrs) -> AssetRef {
    AssetRef::new(image.source().value().to_owned()).unwrap()
}
#[derive(Default)]
struct Sink(RefCell<Option<Result<ResolvedAsset, AssetError>>>);
impl AssetSink for Sink {
    fn resolved(self: Rc<Self>, result: Result<ResolvedAsset, AssetError>) {
        *self.0.borrow_mut() = Some(result);
    }
}
fn resolve(host: &FixtureAssets, image: &ImageAttrs) -> Result<ResolvedAsset, AssetError> {
    let sink = Rc::new(Sink::default());
    host.resolve(reference(image), sink.clone());
    sink.0.borrow_mut().take().unwrap()
}

#[test]
fn image_bytes_survive_undo_redo_and_fresh_host_reopen() {
    let workspace = Workspace::new();
    let path = workspace.document();
    let mut store = FixtureStore::new(path.clone());
    let host = FixtureAssets::for_document(&path);
    let original = demo_fixture();
    let selection = DocumentSelection::collapsed(caret_at_first_block(&original).focus());
    let mut session = DocumentSession::new(original.clone(), selection).unwrap();
    let bytes = png();
    let attrs = host.import_image(AssetFormat::Png, &bytes).unwrap();
    session
        .apply_intent(&EditIntent::InsertImage {
            image: attrs.clone(),
        })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    let inserted = session.document().clone();
    assert_eq!(resolve(&host, &attrs).unwrap().bytes(), bytes);
    session.undo().unwrap();
    assert!(canonical_semantics_equal(session.document(), &original));
    store.save(session.document()).unwrap();
    let undone = FixtureStore::new(path.clone()).load().unwrap().unwrap();
    assert!(
        canonical_semantics_equal(&original, &undone),
        "orphan sidecars cannot resurrect an image"
    );
    assert_eq!(
        resolve(&FixtureAssets::for_document(&path), &attrs)
            .unwrap()
            .bytes(),
        bytes
    );
    session.redo().unwrap();
    assert!(canonical_semantics_equal(session.document(), &inserted));
    store.save(session.document()).unwrap();
    drop(host);
    let reloaded = FixtureStore::new(path.clone()).load().unwrap().unwrap();
    assert!(canonical_semantics_equal(&inserted, &reloaded));
    let reopened_host = FixtureAssets::for_document(&path);
    assert_eq!(resolve(&reopened_host, &attrs).unwrap().bytes(), bytes);
    assert_eq!((attrs.width(), attrs.height()), (Some(8), Some(8)));
}

#[test]
fn failed_import_does_not_mutate_document_selection_or_history() {
    let workspace = Workspace::new();
    let path = workspace.document();
    let host = FixtureAssets::for_document(&path);
    let original = demo_fixture();
    let mut session = DocumentSession::new(
        original.clone(),
        DocumentSelection::collapsed(caret_at_first_block(&original).focus()),
    )
    .unwrap();
    let selection = session.selection();
    std::fs::write(&host.directory, b"not a directory").unwrap();
    for bytes in [vec![0, 1, 2], png()] {
        let result = host.import_image(AssetFormat::Png, &bytes);
        if let Ok(image) = result {
            session
                .apply_intent(&EditIntent::InsertImage { image })
                .unwrap();
            panic!("import should fail");
        }
        assert!(canonical_semantics_equal(session.document(), &original));
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), (0, 0));
    }
}

#[test]
fn missing_corrupt_and_path_escape_assets_fail_closed() {
    let workspace = Workspace::new();
    let host = FixtureAssets::for_document(&workspace.document());
    let attrs = host.import_image(AssetFormat::Png, &png()).unwrap();
    std::fs::write(host.directory.join(attrs.source().value()), b"bad").unwrap();
    assert_eq!(resolve(&host, &attrs), Err(AssetError::InvalidImage));
    std::fs::remove_file(host.directory.join(attrs.source().value())).unwrap();
    assert_eq!(resolve(&host, &attrs), Err(AssetError::NotFound));
    assert_eq!(
        host.read(AssetRef::new("../document.txt".into()).unwrap()),
        Err(AssetError::InvalidRef)
    );
    assert_eq!(
        host.import_image(AssetFormat::Png, &vec![0; MAX_BYTES + 1]),
        Err(AssetError::InvalidImage)
    );
}

#[test]
fn failed_save_leaves_previous_snapshot_intact() {
    let workspace = Workspace::new();
    let path = workspace.document();
    let mut store = FixtureStore::new(path.clone());
    store.save(&demo_fixture()).unwrap();
    let before = std::fs::read(&path).unwrap();
    // A failed replacement must not truncate an existing destination.
    assert!(crate::atomic_file::write(&workspace.0, b"bad").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(store.load().unwrap().is_some());
}

#[test]
fn jpeg_round_trips_and_oversized_dimensions_are_rejected() {
    let workspace = Workspace::new();
    let host = FixtureAssets::for_document(&workspace.document());
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        3,
        2,
        image::Rgb([0, 120, 255]),
    ));
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Jpeg)
        .unwrap();
    let bytes = encoded.into_inner();
    let attrs = host.import_image(AssetFormat::Jpeg, &bytes).unwrap();
    let resolved = resolve(&host, &attrs).unwrap();
    assert_eq!(resolved.format(), AssetFormat::Jpeg);
    assert_eq!(resolved.bytes(), bytes);
    let oversized = image::DynamicImage::ImageLuma8(image::GrayImage::new(4097, 1));
    let mut encoded = Cursor::new(Vec::new());
    oversized
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(
        host.import_image(AssetFormat::Png, &encoded.into_inner()),
        Err(AssetError::InvalidImage)
    );
}
