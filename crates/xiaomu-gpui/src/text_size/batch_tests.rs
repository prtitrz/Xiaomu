use super::*;
use gpui::{TestAppContext, font};
use std::cell::Cell;
use xiaomu_core::document::{InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

fn fixture() -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes = (0..2)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(InlineContent::empty()),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.iter().copied()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

fn base_style() -> TextSizeStyle {
    TextSizeStyle::new(
        font("monospace"),
        FontSizeContext::new(20.0, 16.0, 16.0).unwrap(),
        1.4,
    )
    .with_font_family("serif, monospace")
}

struct Batch {
    calls: Cell<usize>,
    single_calls: Cell<usize>,
    missing: bool,
    extra: bool,
    invalid: bool,
}

impl TextSizeStyleProvider for Batch {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        self.single_calls.set(self.single_calls.get() + 1);
        base_style()
    }

    fn styles_for_document(
        &self,
        document: &XiaomuDocument,
    ) -> Option<BTreeMap<NodeId, TextSizeStyle>> {
        self.calls.set(self.calls.get() + 1);
        let mut styles: BTreeMap<_, _> = document
            .store()
            .iter()
            .filter(|node| node.content().as_inline().is_some())
            .map(|node| (node.id(), base_style()))
            .collect();
        if self.missing {
            styles.pop_first();
        }
        if self.extra {
            styles.insert(document.root(), base_style());
        }
        if self.invalid {
            styles.first_entry().unwrap().get_mut().line_height = f32::NAN;
        }
        Some(styles)
    }
}

fn provider(missing: bool, extra: bool, invalid: bool) -> Rc<Batch> {
    Rc::new(Batch {
        calls: Cell::new(0),
        single_calls: Cell::new(0),
        missing,
        extra,
        invalid,
    })
}

#[gpui::test]
fn batch_is_used_once_without_per_node_fallback_and_normalizes_identically(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let (document, nodes) = fixture();
        let provider = provider(false, false, false);
        let cap = TextSizeCapability::new(window.text_system().clone(), provider.clone());
        cap.validate_document(&document, &InlineAtomRendererRegistry::new())
            .unwrap();
        assert_eq!(provider.calls.get(), 1);
        assert_eq!(provider.single_calls.get(), 0);
        let styles = cap.prepare_styles(&document).unwrap();
        assert_eq!(provider.calls.get(), 2);
        assert_eq!(styles.len(), 2);
        for id in nodes {
            let individual = cap.style(&document, document.node(id).unwrap()).unwrap();
            assert_eq!(styles[&id].base_font(), individual.base_font());
            assert_eq!(styles[&id].context(), individual.context());
            assert_eq!(styles[&id].color(), individual.color());
            assert_eq!(styles[&id].line_height(), individual.line_height());
        }
    });
}

#[gpui::test]
fn batch_coverage_rejects_missing_or_extra_keys_before_normalization(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, nodes) = fixture();
        for (missing, extra, expected) in [(true, false, nodes[0]), (false, true, document.root())]
        {
            let provider = provider(missing, extra, false);
            let cap = TextSizeCapability::new(window.text_system().clone(), provider.clone());
            let error = cap
                .validate_document(&document, &InlineAtomRendererRegistry::new())
                .unwrap_err();
            assert_eq!(error.kind(), TextSizeErrorKind::InvalidStyleMap);
            assert_eq!(error.node(), expected);
            assert_eq!(provider.calls.get(), 1);
            assert_eq!(provider.single_calls.get(), 0);
        }
    });
}

#[gpui::test]
fn batch_still_validates_empty_block_geometry(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, nodes) = fixture();
        let cap =
            TextSizeCapability::new(window.text_system().clone(), provider(false, false, true));
        let error = cap
            .validate_document(&document, &InlineAtomRendererRegistry::new())
            .unwrap_err();
        assert_eq!(error.kind(), TextSizeErrorKind::InvalidStyle);
        assert_eq!(error.node(), nodes[0]);
    });
}

#[test]
fn caret_context_distinguishes_inherited_and_each_explicit_probe() {
    let context = TextSizeCaretContext::new(20.0, None, Some(48.0), Some(12.0));
    assert_eq!(context.effective_size(), 20.0);
    assert_eq!(context.stored_size(), None);
    assert_eq!(context.before_size(), Some(48.0));
    assert_eq!(context.after_size(), Some(12.0));
}
