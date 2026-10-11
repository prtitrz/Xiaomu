//! Synthetic native geometry fixture in the existing harness.
//! Run with `--first-line-inset [--first-line-width=150]`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Application, Bounds, Context, Entity, IntoElement, KeyBinding, Render, TitlebarOptions,
    Window, WindowBounds, WindowOptions, actions, div, font, prelude::*, px, size,
};
use xiaomu_core::document::{AttrValue, Node, XiaomuDocument};
use xiaomu_gpui::block_alignment::{BlockAlignment, BlockAlignmentProvider};
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_gpui::first_line_inset::FirstLineInsetProvider;
use xiaomu_gpui::font_size::FontSizeContext;
use xiaomu_gpui::inline_atom::InlineAtomRendererRegistry;
use xiaomu_gpui::text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider};
use xiaomu_runtime::persistence::DocumentPersistence;
use xiaomu_runtime::session::{DocumentSelection, PolicyError, SessionPolicy};

actions!(first_line_fixture, [CycleWidth]);

#[derive(Default)]
struct FixtureGeometry;

impl FirstLineInsetProvider for FixtureGeometry {
    fn first_line_inset(&self, _: &XiaomuDocument, node: &Node, _: f32) -> f32 {
        match node.attrs().get("harnessInset") {
            Some(AttrValue::Integer(value)) => *value as f32,
            _ => 0.0,
        }
    }
}

impl BlockAlignmentProvider for FixtureGeometry {
    fn alignment(&self, node: &Node) -> BlockAlignment {
        match node.attrs().get("harnessAlign") {
            Some(AttrValue::String(value)) if value == "center" => BlockAlignment::Center,
            Some(AttrValue::String(value)) if value == "right" => BlockAlignment::Right,
            _ => BlockAlignment::Left,
        }
    }
}

struct FixedStyle;
impl TextSizeStyleProvider for FixedStyle {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(18.0, 16.0, 16.0).expect("fixed font bases"),
            1.8,
        )
    }
}

struct GeometryPolicy(Rc<TextSizeCapability>);
impl SessionPolicy for GeometryPolicy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .validate_document(document, &InlineAtomRendererRegistry::new())
            .map_err(|error| PolicyError::new(error.to_string()))
    }
}

struct FixtureHost {
    editor: Entity<DocumentView>,
    text_width: f32,
}

impl FixtureHost {
    fn cycle_width(&mut self, _: &CycleWidth, _: &mut Window, cx: &mut Context<Self>) {
        const WIDTHS: [f32; 8] = [360.0, 150.0, 60.0, 40.0, 36.0, 35.0, 20.0, 1.0];
        let next = WIDTHS
            .iter()
            .position(|width| *width == self.text_width)
            .map_or(0, |index| (index + 1) % WIDTHS.len());
        self.text_width = WIDTHS[next];
        eprintln!(
            "xiaomu: first-line fixture text width {}px",
            self.text_width
        );
        cx.notify();
    }
}

impl Render for FixtureHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // DocumentView has p_4 (16px per side at the fixed 16px rem size).
        // Keep a fixed pane so even 1px text boxes are observable in a usable window.
        div()
            .size_full()
            .on_action(cx.listener(Self::cycle_width))
            .bg(gpui::rgb(0xe8edf2))
            .flex()
            .flex_col()
            .child(
                div().h(px(54.0)).px_4().py_2().text_size(px(14.0)).child(format!(
                    "First-line inset fixture | text box {}px | base 18px | inset 36px | Ctrl+Alt+W cycles widths",
                    self.text_width
                )),
            )
            .child(
                div()
                    .w(px(self.text_width + 32.0))
                    .flex_1()
                    .min_h(px(0.0))
                    .child(self.editor.clone()),
            )
    }
}

pub(super) fn run(args: &[String]) {
    let width = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--first-line-width="))
        .map(|value| {
            value
                .parse::<f32>()
                .expect("--first-line-width must be numeric")
        })
        .unwrap_or(360.0);
    assert!(
        width.is_finite() && (1.0..=1200.0).contains(&width),
        "fixture width must be 1..=1200px"
    );
    let path = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("xiaomu-first-line-fixture.txt"));
    let store = Rc::new(RefCell::new(super::store::FixtureStore::new(path.clone())));
    let fixture_source = initial_source(args);
    let document = store
        .borrow()
        .load()
        .expect("refusing corrupt first-line fixture")
        .unwrap_or_else(|| {
            super::store::parse_document(&fixture_source).expect("checked-in first-line fixture")
        });
    eprintln!(
        "xiaomu: synthetic first-line fixture, text width {width}px; Ctrl+S saves {}",
        path.display()
    );
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                // Do not quit synchronously inside the X11 close callback.
                cx.spawn(async move |cx| {
                    let _ = cx.update(|cx| {
                        if cx.windows().is_empty() {
                            cx.quit();
                        }
                    });
                })
                .detach();
            }
        })
        .detach();
        bind_default_editor_keys(cx);
        cx.bind_keys([KeyBinding::new("ctrl-alt-w", CycleWidth, None)]);
        let bounds = Bounds::centered(None, size(px((width + 64.0).max(760.0)), px(800.0)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Xiaomu - first-line inset fixture".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_rem_size(px(16.0));
                    let geometry = Rc::new(FixtureGeometry);
                    let capability = Rc::new(
                        TextSizeCapability::new(window.text_system().clone(), Rc::new(FixedStyle))
                            .with_first_line_inset_provider(geometry.clone()),
                    );
                    let selection =
                        DocumentSelection::text(super::store::caret_at_first_block(&document));
                    let instance = EditorInstance::new_with_policy(
                        document,
                        selection,
                        EditorHooks {
                            persistence: Some(store.clone()),
                            ..EditorHooks::default()
                        },
                        Box::new(GeometryPolicy(capability.clone())),
                    )
                    .expect("synthetic fixture must pass native geometry admission")
                    .with_text_size_capability(capability)
                    .with_block_alignment_provider(geometry);
                    let editor = cx.new(|_| instance.build_view());
                    cx.new(|_| FixtureHost {
                        editor,
                        text_width: width,
                    })
                },
            )
            .expect("open first-line fixture window");
        handle
            .update(cx, |host, window, cx| {
                host.editor
                    .update(cx, |view, cx| view.focus_selection(window, cx));
                cx.activate(true);
            })
            .expect("focus first-line fixture");
    });
}

/// Selecting one case removes its label paragraph so very narrow boxes remain usable.
/// Existing persisted documents always take precedence over this initial seed.
fn initial_source(args: &[String]) -> String {
    let source = include_str!("../fixtures/first-line-inset.txt");
    let Some(case) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--first-line-case="))
    else {
        return source.to_owned();
    };
    let case = case
        .parse::<usize>()
        .expect("--first-line-case must be 1..=12");
    assert!((1..=12).contains(&case), "--first-line-case must be 1..=12");
    let lines: Vec<_> = source.lines().skip(1 + (case - 1) * 3).take(3).collect();
    format!("xiaomu-fixture-doc v5\n{}\n{}\n", lines[1], lines[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_checked_in_case_parses_with_persistent_geometry() {
        let full = initial_source(&[]);
        let document = super::super::store::parse_document(&full).unwrap();
        assert_eq!(document.store().iter().count(), 25);
        for case in 1..=12 {
            let source = initial_source(&[format!("--first-line-case={case}")]);
            let document = super::super::store::parse_document(&source).unwrap();
            let paragraph = document
                .store()
                .iter()
                .find(|node| node.content().as_inline().is_some())
                .unwrap();
            assert_eq!(
                FixtureGeometry.first_line_inset(&document, paragraph, 18.0),
                if case == 1 { 0.0 } else { 36.0 }
            );
        }
    }
}
