use super::*;
use crate::first_line_inset::FirstLineInsetProvider;
use gpui::{TestAppContext, font};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, PolicyError, SessionPolicy,
};

struct Style;
impl TextSizeStyleProvider for Style {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(18.0, 16.0, 16.0).unwrap(),
            1.5,
        )
    }
}
struct Inset(f32);
impl FirstLineInsetProvider for Inset {
    fn first_line_inset(&self, document: &XiaomuDocument, node: &Node, base_font_px: f32) -> f32 {
        assert!(document.node(node.id()).is_some());
        assert_eq!(base_font_px, 18.0);
        self.0
    }
}
struct Policy(TextSizeCapability);
impl SessionPolicy for Policy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .validate_document(document, &InlineAtomRendererRegistry::new())
            .map_err(|error| PolicyError::new(error.to_string()))
    }
}
fn fixture(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
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
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

#[gpui::test]
fn first_line_provider_invalid_values_reject_before_document_admission(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, node) = fixture("plain");
        for inset in [
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            65_537.0,
        ] {
            let cap = TextSizeCapability::new(window.text_system().clone(), Rc::new(Style))
                .with_first_line_inset_provider(Rc::new(Inset(inset)));
            let error = cap
                .validate_document(&document, &InlineAtomRendererRegistry::new())
                .unwrap_err();
            assert_eq!(error.node(), node);
            assert_eq!(error.kind(), TextSizeErrorKind::InvalidFirstLineInset);
        }
    });
}

#[gpui::test]
fn first_line_uniform_candidate_edit_is_atomic_and_zero_keeps_unicode(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for inset in [0.0, -0.0, 36.0] {
            let (document, node) = fixture("Latin");
            let point = TextPoint::new(
                node,
                document
                    .node(node)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(5)
                    .unwrap(),
                CursorAffinity::Before,
            );
            let cap = TextSizeCapability::new(window.text_system().clone(), Rc::new(Style))
                .with_first_line_inset_provider(Rc::new(Inset(inset)));
            let mut session = DocumentSession::new_with_policy(
                document.clone(),
                DocumentSelection::collapsed(point),
                Box::new(Policy(cap)),
            )
            .unwrap();
            let selection = session.selection();
            let history = session.history_depths();
            let result = session.apply_intent(&EditIntent::InsertText {
                text: "عربي".to_owned(),
            });
            if inset > 0.0 {
                assert!(result.is_err());
                assert_eq!(session.document().node(node), document.node(node));
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), history);
                session
                    .apply_intent(&EditIntent::InsertText {
                        text: " 中文".to_owned(),
                    })
                    .unwrap();
                session.undo().unwrap();
                assert_eq!(session.document().node(node), document.node(node));
                session.redo().unwrap();
            } else {
                assert!(result.is_ok());
            }
        }
    });
}
