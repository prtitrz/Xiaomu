//! Virtual backend coverage: these do not certify real-font or native IME parity.

use super::*;
use crate::block_view::{FontCatalog, text_runs};
use crate::font_size::FontSizeError;
use crate::inline_atom::{InlineAtomRenderer, InlineAtomView};
use gpui::{FontStyle, FontWeight, TestAppContext, font};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, TextStyleAttributes, TextStyleMark,
};

struct FixedStyle {
    line_height: f32,
    caret: Option<f32>,
}

impl Default for FixedStyle {
    fn default() -> Self {
        Self {
            line_height: 1.5,
            caret: None,
        }
    }
}

impl TextSizeStyleProvider for FixedStyle {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(18.0, 16.0, 20.0).unwrap(),
            self.line_height,
        )
        .with_color(gpui::rgba(0x123456ff).into())
    }

    fn caret_height(&self, _: TextSizeCaretContext) -> Option<f32> {
        self.caret
    }
}

fn size_marks(value: StringAttribute) -> MarkSet {
    MarkSet::new([Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(value),
    ))])
    .unwrap()
}

fn fixture(parts: &[(&str, MarkSet)]) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new(
                    parts
                        .iter()
                        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap()),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    finish(builder, node)
}

fn finish(mut builder: NodeStoreBuilder, node: NodeId) -> (XiaomuDocument, NodeId) {
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

fn capability(system: &Arc<WindowTextSystem>) -> TextSizeCapability {
    TextSizeCapability::new(system.clone(), Rc::new(FixedStyle::default()))
}

#[gpui::test]
fn canonical_sizes_and_marks_resolve_identically_for_admission_and_layout(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let cap = capability(window.text_system());
        let styled = MarkSet::new([
            Mark::Bold,
            Mark::Italic,
            Mark::Underline,
            Mark::Strike,
            Mark::TextStyle(TextStyleMark::from_attributes(
                TextStyleAttributes::default()
                    .with_font_size("2rem".into())
                    .with_font_family("serif, monospace".into())
                    .with_color("#000000".into()),
            )),
        ])
        .unwrap();
        let (document, id) = fixture(&[
            ("small ", size_marks("50%".into())),
            ("BIG ", styled),
            ("inherited", size_marks(StringAttribute::Null)),
        ]);
        let original = document.clone();
        let registry = InlineAtomRendererRegistry::new();
        cap.validate_document(&document, &registry).unwrap();
        let node = document.node(id).unwrap();
        let projection = InlineAtomDisplayProjection::build(&document, id, &registry).unwrap();
        let (text, segments) =
            project_atom_display_content(node.content().as_inline().unwrap(), &projection);
        let style = cap.style(&document, node).unwrap();
        let resolved = cap.resolve_segments(id, &style, &segments).unwrap();
        assert_eq!(
            resolved
                .sizes
                .iter()
                .map(|span| span.size)
                .collect::<Vec<_>>(),
            vec![px(9.0), px(32.0), px(18.0)]
        );
        let expected = text_runs(
            &segments,
            style.base_font().clone(),
            style.color(),
            &FontCatalog::from_system(window.text_system()),
        );
        assert_eq!(resolved.runs, expected);
        assert_eq!(resolved.runs[0].color, style.color());
        assert_ne!(style.color(), black());
        assert_eq!(resolved.runs[1].color, black());
        assert_eq!(resolved.runs[1].underline.unwrap().color, Some(black()));
        assert_eq!(resolved.runs[1].strikethrough.unwrap().color, Some(black()));
        assert_eq!(resolved.runs[2].color, style.color());
        assert_eq!(resolved.runs[1].font.weight, FontWeight::BOLD);
        assert_eq!(resolved.runs[1].font.style, FontStyle::Italic);
        for width in [1.0, 47.0, 900.0] {
            let input = resolved.input(&text, &style, px(width));
            assert_eq!(input.base_color, style.color());
            assert_eq!(input.runs, expected.as_slice());
            assert_eq!(
                mixed_size::admission(cap.text_system(), input.clone()).unwrap(),
                mixed_size::Admission::MixedLtr
            );
            assert!(matches!(
                mixed_size::layout(cap.text_system(), input).unwrap(),
                mixed_size::Layout::Mixed(_)
            ));
        }
        assert_eq!(document.store(), original.store());
        assert_eq!(document.revision(), original.revision());
        assert_eq!(document.root(), original.root());
        assert!(Arc::ptr_eq(&cap.system, &cap.clone().system));
        assert!(Rc::ptr_eq(&cap.provider, &cap.clone().provider));
    });
}

#[gpui::test]
fn all_canonical_size_states_fail_closed_without_rewriting(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let cap = capability(window.text_system());
        for value in [
            "calc(12px + 2px)",
            "var(--size)",
            "0px",
            "513px",
            "10vw",
            "-2em",
            "12px junk",
        ] {
            let (document, id) = fixture(&[("text", size_marks(value.into()))]);
            let original = document.clone();
            let error = cap
                .validate_document(&document, &InlineAtomRendererRegistry::new())
                .unwrap_err();
            assert_eq!(error.node(), id);
            assert_eq!(error.display_range(), &(0..4));
            assert!(matches!(error.kind(), TextSizeErrorKind::FontSize(_)));
            assert_eq!(document.store(), original.store());
            assert_eq!(document.revision(), original.revision());
            assert_eq!(document.root(), original.root());
        }
        for value in [
            StringAttribute::Missing,
            StringAttribute::Null,
            "".into(),
            " /*empty*/ ".into(),
            "inherit".into(),
        ] {
            let (document, _) = fixture(&[("text", size_marks(value))]);
            cap.validate_document(&document, &InlineAtomRendererRegistry::new())
                .unwrap();
        }
    });
}

#[gpui::test]
fn mixed_script_seams_and_work_limits_do_not_restrict_uniform_native_text(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let cap = capability(window.text_system());
        let registry = InlineAtomRendererRegistry::new();
        let (document, _) = fixture(&[("العربية हिन्दी", size_marks("24px".into()))]);
        cap.validate_document(&document, &registry).unwrap();
        let (document, _) = fixture(&[
            ("العربية", size_marks("24px".into())),
            (" text", MarkSet::empty()),
        ]);
        assert_eq!(
            cap.validate_document(&document, &registry)
                .unwrap_err()
                .kind(),
            TextSizeErrorKind::ComplexScriptOrControl
        );
        let (document, _) = fixture(&[
            ("e", size_marks("24px".into())),
            ("\u{301}", MarkSet::empty()),
        ]);
        assert_eq!(
            cap.validate_document(&document, &registry)
                .unwrap_err()
                .kind(),
            TextSizeErrorKind::SizeBoundaryInsideGrapheme
        );
        let long = "x".repeat(2048);
        let (document, _) = fixture(&[(&long, size_marks("24px".into()))]);
        cap.validate_document(&document, &registry).unwrap();
        let (document, _) = fixture(&[(&long, size_marks("24px".into())), ("y", MarkSet::empty())]);
        assert_eq!(
            cap.validate_document(&document, &registry)
                .unwrap_err()
                .kind(),
            TextSizeErrorKind::WorkBudgetExceeded
        );
    });
}

struct Label(&'static str);
impl InlineAtomRenderer for Label {
    fn display_text(&self, _: &InlineAtomView) -> String {
        self.0.into()
    }
}

fn atom_fixture(kind: AtomKind, marks: MarkSet) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let content = if kind.is_hard_break() {
        InlineAtomContent::hard_break()
    } else {
        InlineAtomContent::new("fallback").unwrap()
    };
    let atom = builder
        .insert(
            NodeKind::InlineAtom(kind),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(content.with_marks(marks)),
        )
        .unwrap();
    let inline = InlineContent::new([TextRun::new("AB", MarkSet::empty()).unwrap()]).unwrap();
    let offset = inline.offset_at(1).unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    inline.runs().iter().cloned(),
                    [InlineAtomPlacement::new(atom, offset)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    finish(builder, node)
}

#[gpui::test]
fn exact_registered_atom_labels_and_marked_hard_breaks_participate_in_admission(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let cap = capability(window.text_system());
        let atom_kind = AtomKind::new("label").unwrap();
        let (document, id) = atom_fixture(atom_kind.clone(), size_marks("24px".into()));
        let mut registry = InlineAtomRendererRegistry::new();
        cap.validate_document(&document, &registry).unwrap();
        registry.register(&atom_kind, Rc::new(Label("العربية")));
        let error = cap.validate_document(&document, &registry).unwrap_err();
        assert_eq!(error.node(), id);
        assert_eq!(error.kind(), TextSizeErrorKind::ComplexScriptOrControl);
        registry.register(&atom_kind, Rc::new(Label("")));
        cap.validate_document(&document, &registry).unwrap();
        let (document, id) = atom_fixture(AtomKind::hard_break(), size_marks("48px".into()));
        cap.validate_document(&document, &registry).unwrap();
        let projection = InlineAtomDisplayProjection::build(&document, id, &registry).unwrap();
        assert_eq!(projection.display_text(), "A\nB");
        let (document, _) = atom_fixture(AtomKind::hard_break(), size_marks("calc(1px)".into()));
        let error = cap.validate_document(&document, &registry).unwrap_err();
        assert_eq!(error.display_range(), &(1..2));
        assert_eq!(
            error.kind(),
            TextSizeErrorKind::FontSize(FontSizeError::UnsupportedCss)
        );
    });
}

#[gpui::test]
fn invalid_host_geometry_and_caret_metrics_reject_even_empty_blocks(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, _) = fixture(&[]);
        let registry = InlineAtomRendererRegistry::new();
        for line_height in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
            let cap = TextSizeCapability::new(
                window.text_system().clone(),
                Rc::new(FixedStyle {
                    line_height,
                    caret: None,
                }),
            );
            assert_eq!(
                cap.validate_document(&document, &registry)
                    .unwrap_err()
                    .kind(),
                TextSizeErrorKind::InvalidStyle
            );
        }
        for caret in [
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            MAX_CARET_HEIGHT_PX + 1.0,
        ] {
            let cap = TextSizeCapability::new(
                window.text_system().clone(),
                Rc::new(FixedStyle {
                    line_height: 1.5,
                    caret: Some(caret),
                }),
            );
            assert_eq!(
                cap.validate_document(&document, &registry)
                    .unwrap_err()
                    .kind(),
                TextSizeErrorKind::InvalidStyle
            );
        }
        let cap = TextSizeCapability::new(
            window.text_system().clone(),
            Rc::new(FixedStyle {
                line_height: 1.5,
                caret: Some(25.0),
            }),
        );
        cap.validate_document(&document, &registry).unwrap();
    });
}

struct AncestorStyle;
impl TextSizeStyleProvider for AncestorStyle {
    fn style(&self, document: &XiaomuDocument, node: &Node) -> TextSizeStyle {
        let in_quote = document
            .parent_of(node.id())
            .is_some_and(|id| matches!(document.node(id).unwrap().kind(), NodeKind::Quote));
        TextSizeStyle::new(
            font("monospace"),
            FontSizeContext::new(if in_quote { 13.0 } else { 20.0 }, 16.0, 16.0).unwrap(),
            1.55,
        )
        .with_font_family("serif, monospace")
    }
}

#[gpui::test]
fn provider_receives_candidate_ancestors_and_shared_family_resolver(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut builder = NodeStoreBuilder::new();
        let node = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::empty()),
            )
            .unwrap();
        let quote = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([node]),
            )
            .unwrap();
        let (document, _) = finish(builder, quote);
        let cap = TextSizeCapability::new(window.text_system().clone(), Rc::new(AncestorStyle));
        cap.validate_document(&document, &InlineAtomRendererRegistry::new())
            .unwrap();
        let style = cap.style(&document, document.node(node).unwrap()).unwrap();
        assert_eq!(style.context().parent_px(), 13.0);
        assert_eq!(
            style.base_font(),
            &FontCatalog::from_system(window.text_system())
                .apply("serif, monospace", &font("monospace"))
        );
    });
}
