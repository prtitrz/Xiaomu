//! Xiaomu editor harness: real-machine verification entry point.
//!
//! P2.6 minimal host-contract harness over the multi-block editor:
//!
//! ```text
//! create editor   -> run_document_editor_with_hooks
//! load document   -> FixtureStore::load (file-backed adapter, harness-
//!                    internal format, not a codec commitment)
//! listen          -> ChangeCounter implements DocumentChangeListener
//! persist         -> Ctrl/Cmd-S writes the canonical snapshot through the
//!                    DocumentPersistence seam
//! ```
//!
//! Usage: `editor_harness [store-path]` (default `./xiaomu-harness-store.txt`).
//! Delete the file to start from the built-in demo fixture again.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

mod store;

use xiaomu_core::document::XiaomuDocument;
use xiaomu_gpui::editor::{EditorHooks, run_document_editor_with_hooks};
use xiaomu_runtime::persistence::DocumentPersistence;
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSelection};

use store::{FixtureStore, caret_at_first_block, demo_fixture};

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("xiaomu-harness-store.txt"));
    let store = Rc::new(RefCell::new(FixtureStore::new(path.clone())));

    // load document: through the adapter when a snapshot exists, otherwise
    // start from the in-memory demo fixture.
    let (document, source) = match store.borrow().load() {
        Some(document) => (document, format!("loaded from {}", path.display())),
        None => (demo_fixture(), "created demo fixture".to_owned()),
    };
    eprintln!("xiaomu: {source}");

    let selection = caret_at_first_block(&document);
    let counter = Rc::new(RefCell::new(ChangeCounter::default()));

    let hooks = EditorHooks {
        persistence: Some(store.clone()),
        listener: Some(Box::new(CounterListener(counter.clone()))),
    };

    // The counter survives the run because GPUI quits when the window
    // closes; report what the listen leg observed.
    let result = run_document_editor_with_hooks(document, selection, hooks);
    let edits = counter.borrow().document_changes;
    eprintln!("xiaomu: session ended after {edits} committed changes");
    if let Err(error) = result {
        eprintln!("xiaomu: failed to start editor: {error}");
        std::process::exit(1);
    }
}

/// Counts committed document changes observed through the listen seam.
#[derive(Default)]
struct ChangeCounter {
    document_changes: u64,
}

struct CounterListener(Rc<RefCell<ChangeCounter>>);

impl DocumentChangeListener for CounterListener {
    fn document_changed(&mut self, _document: &XiaomuDocument, _selection: DocumentSelection) {
        self.0.borrow_mut().document_changes += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{escape_text, parse_document, structurally_equal, unescape_text};
    use xiaomu_core::document::{NodeContent as TestNodeContent, NodeKind as TestNodeKind};

    #[test]
    fn fixture_round_trips_through_the_adapter() {
        let original = demo_fixture();

        let path =
            std::env::temp_dir().join(format!("xiaomu-harness-test-{}.txt", std::process::id()));
        let mut adapter = FixtureStore::new(path.clone());
        adapter.save(&original).expect("save");

        let restored = adapter.load().expect("load after save");
        let _ = std::fs::remove_file(&path);

        assert!(structurally_equal(&original, &restored));
    }

    #[test]
    fn load_without_store_file_starts_from_none() {
        let adapter = FixtureStore::new(std::env::temp_dir().join("xiaomu-harness-missing"));
        assert!(adapter.load().is_none());
    }

    #[test]
    fn escapes_tabs_and_backslashes_in_leaf_text() {
        assert_eq!(escape_text("a\\b\tc"), "a\\\\b\\tc");
        assert_eq!(unescape_text("a\\\\b\\tc"), "a\\b\tc");
    }

    #[test]
    fn parses_headings_quotes_and_lists() {
        let text = "xiaomu-fixture-doc v1\nh2\t标题\np\t正文\nquote\np\t引文\nend\nul\nli\np\t甲\nend\nli\np\t乙\nend\nend\n";
        let document = parse_document(text).expect("parse");

        let root_children = match document.node(document.root()).unwrap().content() {
            TestNodeContent::Children(children) => children.clone(),
            _ => panic!("root must have children"),
        };
        assert_eq!(root_children.len(), 4);
        let kinds: Vec<&TestNodeKind> = root_children
            .iter()
            .map(|id| document.node(*id).unwrap().kind())
            .collect();
        assert!(matches!(kinds[0], TestNodeKind::Heading(_)));
        assert!(matches!(kinds[1], TestNodeKind::Paragraph));
        assert!(matches!(kinds[2], TestNodeKind::Quote));
        assert!(matches!(kinds[3], TestNodeKind::BulletList));
    }

    #[test]
    fn rejects_unknown_headers_and_unclosed_containers() {
        assert!(parse_document("nope\n").is_err());
        assert!(parse_document("xiaomu-fixture-doc v1\nquote\n").is_err());
    }
}
