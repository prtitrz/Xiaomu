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

use xiaomu_core::document::{NodeContent, NodeId, XiaomuDocument};
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
    // start from the in-memory demo fixture. A corrupt store is a hard
    // failure: never silently replace persisted data with a new document.
    let (document, source) = match store.borrow().load() {
        Ok(Some(document)) => (document, format!("loaded from {}", path.display())),
        Ok(None) => (demo_fixture(), "created demo fixture".to_owned()),
        Err(error) => {
            eprintln!("xiaomu: failed to load persisted document: {error}");
            eprintln!("xiaomu: refusing to start a new document over corrupt store data");
            std::process::exit(1);
        }
    };
    eprintln!("xiaomu: {source}");
    print_outline(&document);

    let selection = caret_at_first_block(&document);
    let counter = Rc::new(RefCell::new(ChangeCounter::default()));

    let hooks = EditorHooks {
        persistence: Some(store.clone()),
        listener: Some(Box::new(CounterListener(counter.clone()))),
        atom_renderers: None,
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

/// Prints the loaded document's block outline so the operator can see what
/// structure the session actually holds (kinds only, one line per node).
fn print_outline(document: &XiaomuDocument) {
    fn walk(document: &XiaomuDocument, id: NodeId, depth: usize, out: &mut String) {
        let Some(node) = document.node(id) else {
            return;
        };
        let indent = "  ".repeat(depth);
        let label = match node.content() {
            NodeContent::Inline(inline) => {
                let text: String = inline
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect();
                let preview: String = text.chars().take(12).collect();
                format!("{:?} \u{201c}{preview}\u{201d}", node.kind())
            }
            _ => format!("{:?}", node.kind()),
        };
        out.push_str(&format!("  outline: {indent}{label}\n"));
        if let NodeContent::Children(children) = node.content() {
            for child in children {
                walk(document, *child, depth + 1, out);
            }
        }
    }
    let mut out = String::new();
    walk(document, document.root(), 0, &mut out);
    eprint!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{canonical_semantics_equal, escape_text, parse_document, unescape_text};
    use xiaomu_core::document::{NodeContent as TestNodeContent, NodeKind as TestNodeKind};

    #[test]
    fn fixture_round_trips_through_the_adapter() {
        let original = demo_fixture();

        let path =
            std::env::temp_dir().join(format!("xiaomu-harness-test-{}.txt", std::process::id()));
        let mut adapter = FixtureStore::new(path.clone());
        adapter.save(&original).expect("save");

        let restored = adapter
            .load()
            .expect("load after save")
            .expect("saved snapshot must exist");
        let _ = std::fs::remove_file(&path);

        assert!(canonical_semantics_equal(&original, &restored));
    }

    #[test]
    fn load_without_store_file_starts_from_none() {
        let adapter = FixtureStore::new(std::env::temp_dir().join("xiaomu-harness-missing"));
        assert!(adapter.load().expect("missing store is Ok(None)").is_none());
    }

    #[test]
    fn load_returns_err_for_corrupt_store_data() {
        let path =
            std::env::temp_dir().join(format!("xiaomu-harness-corrupt-{}", std::process::id()));
        std::fs::write(&path, "not a fixture\n").expect("write corrupt store");
        let adapter = FixtureStore::new(path.clone());
        let result = adapter.load();
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "corrupt data must not look like Ok(None)");
    }

    #[test]
    fn save_rejects_atomic_nodes_instead_of_dropping_them() {
        use xiaomu_core::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

        let mut builder = NodeStoreBuilder::new();
        let rule = builder
            .insert(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([rule]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let path = std::env::temp_dir().join(format!(
            "xiaomu-harness-unsupported-atomic-{}.txt",
            std::process::id()
        ));
        let mut adapter = FixtureStore::new(path.clone());

        let error = adapter
            .save(&document)
            .expect_err("unsupported atomic node must fail closed");
        let _ = std::fs::remove_file(&path);

        assert!(error.0.contains("HorizontalRule"));
        assert!(error.0.contains("refusing to save a lossy snapshot"));
    }

    #[test]
    fn save_rejects_custom_nodes_instead_of_dropping_them() {
        use xiaomu_core::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

        let mut builder = NodeStoreBuilder::new();
        let custom = builder
            .insert(
                NodeKind::custom("fixture-unsupported").unwrap(),
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([custom]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let path = std::env::temp_dir().join(format!(
            "xiaomu-harness-unsupported-custom-{}.txt",
            std::process::id()
        ));
        let mut adapter = FixtureStore::new(path.clone());

        let error = adapter
            .save(&document)
            .expect_err("unsupported custom node must fail closed");
        let _ = std::fs::remove_file(&path);

        assert!(error.0.contains("fixture-unsupported"));
        assert!(error.0.contains("refusing to save a lossy snapshot"));
    }

    #[test]
    fn escapes_tabs_and_backslashes_in_leaf_text() {
        assert_eq!(escape_text("a\\b\tc"), "a\\\\b\\tc");
        assert_eq!(unescape_text("a\\\\b\\tc"), "a\\b\tc");
    }

    #[test]
    fn parses_headings_quotes_and_lists() {
        let text = "xiaomu-fixture-doc v2\nh2\t标题\t\np\t正文\t\nquote\np\t引文\t\nend\nul\nli\np\t甲\t\nend\nli\np\t乙\t\nend\nend\n";
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
        assert!(parse_document("xiaomu-fixture-doc v2\nquote\n").is_err());
    }

    #[test]
    fn save_load_preserves_marks_runs_and_link_attrs() {
        use std::collections::BTreeMap;
        use xiaomu_core::document::{
            AttrValue, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind,
            NodeStoreBuilder, TextRun,
        };

        let marks = |list: Vec<Mark>| MarkSet::new(list).unwrap();
        let run = |text: &str, list: Vec<Mark>| TextRun::new(text, marks(list)).unwrap();
        let mut builder = NodeStoreBuilder::new();
        let attrs = NodeAttrs::new(BTreeMap::from([(
            "note".to_owned(),
            AttrValue::String("keep-me".to_owned()),
        )]))
        .unwrap();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                attrs,
                NodeContent::Inline(
                    InlineContent::new([
                        run("bold ", vec![Mark::Bold]),
                        run("italic ", vec![Mark::Italic]),
                        run("code ", vec![Mark::Code]),
                        run("under ", vec![Mark::Underline]),
                        run("strike ", vec![Mark::Strike]),
                        run(
                            "link",
                            vec![Mark::Link(LinkMark::new(
                                "https://example.com",
                                Some("Example".to_owned()),
                            ))],
                        ),
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let original = XiaomuDocument::new(root, builder.finish()).unwrap();

        let path =
            std::env::temp_dir().join(format!("xiaomu-harness-marks-{}.txt", std::process::id()));
        let mut adapter = FixtureStore::new(path.clone());
        adapter.save(&original).expect("save");
        let restored = adapter.load().expect("load").expect("snapshot present");
        let _ = std::fs::remove_file(&path);

        assert!(
            canonical_semantics_equal(&original, &restored),
            "save/load dropped marks, runs, or attrs"
        );
    }
}
