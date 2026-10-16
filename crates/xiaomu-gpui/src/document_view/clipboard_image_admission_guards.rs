//! Guard and transport priority tests share the synthetic source-admission host.
use super::*;
use gpui::EntityInputHandler;
use xiaomu_core::document::AttrValue;
use xiaomu_runtime::clipboard::encode_metadata;

#[gpui::test]
fn image_admission_never_receives_text_structured_or_invalid_native_metadata(
    cx: &mut TestAppContext,
) {
    for transport in 0..3 {
        let m = mount(cx, Target::Range, Decision::Allow, false, false);
        let item = match transport {
            0 => ClipboardItem::new_string("text".into()),
            1 => {
                let slice = m.session.borrow().clipboard_slice().unwrap().unwrap();
                ClipboardItem::new_string_with_metadata(
                    slice.plain_text().into(),
                    encode_metadata(&slice).unwrap(),
                )
            }
            _ => ClipboardItem::new_string_with_metadata(
                "fallback".into(),
                "xiaomu.clipboard.v14\ninvalid".into(),
            ),
        };
        let (events, _subscription) = events(&m, cx);
        cx.update(|cx| cx.write_to_clipboard(item));
        cx.simulate_keystrokes(m.window.into(), "ctrl-v");
        cx.background_executor.run_until_parked();
        assert!(m.seen.borrow().admitted.is_empty());
        assert_eq!(m.seen.borrow().imports, 0);
        assert!(m.seen.borrow().planned.is_empty());
        assert_eq!(m.seen.borrow().ordinary, usize::from(transport == 0));
        if transport == 2 {
            assert_eq!(m.session.borrow().history_depths(), (0, 0));
            assert_eq!(events.borrow().len(), 1);
            assert_eq!(
                events.borrow()[0].reason(),
                EditorRejectionReason::ClipboardMetadata
            );
        } else {
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            assert!(events.borrow().is_empty());
        }
    }
}

#[gpui::test]
fn image_admission_composition_stops_before_callback_and_import(cx: &mut TestAppContext) {
    let m = mount(cx, Target::Caret, Decision::Allow, false, false);
    image_clipboard(cx);
    m.window
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert!(child.is_composing());
            });
            view.paste(&ClipboardPaste, window, cx);
            assert!(m.seen.borrow().admitted.is_empty());
            assert_eq!(m.seen.borrow().imports, 0);
            assert_eq!(m.session.borrow().history_depths(), (0, 0));
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
            });
        })
        .unwrap();
}

#[gpui::test]
fn image_admission_hidden_table_preimport_guard_preserves_default_order(cx: &mut TestAppContext) {
    for decision in [Decision::Default, Decision::Allow] {
        let mut builder = NodeStoreBuilder::new();
        let head = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let cell = builder
            .insert(
                NodeKind::TableCell,
                NodeAttrs::new([("colspan".into(), AttrValue::Integer(2))].into()).unwrap(),
                NodeContent::children([head]),
            )
            .unwrap();
        let row = builder
            .insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([cell]),
            )
            .unwrap();
        let table = builder
            .insert(
                NodeKind::Table,
                NodeAttrs::empty(),
                NodeContent::children([row]),
            )
            .unwrap();
        let tail = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([table, tail]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(head));
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = Rc::new(Host {
            seen: seen.clone(),
            during_import: RefCell::new(None),
            remote: Cell::new(false),
        });
        let editor = EditorInstance::new_with_policy(
            document,
            selection,
            EditorHooks {
                asset_service: Some(host.clone()),
                ..Default::default()
            },
            Box::new(Planner {
                seen: seen.clone(),
                tail,
                reject: false,
            }),
        )
        .unwrap()
        .with_command_router(Rc::new(Router {
            decision,
            seen: seen.clone(),
        }));
        let session = editor.session().clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
                .unwrap()
        });
        image_clipboard(cx);
        window
            .update(cx, |view, window, cx| {
                assert!(view.selection_has_hidden_table_endpoint());
                view.paste(&ClipboardPaste, window, cx);
            })
            .unwrap();
        assert_eq!(seen.borrow().admitted.len(), 1);
        assert_eq!(
            seen.borrow().imports,
            usize::from(matches!(decision, Decision::Default))
        );
        assert!(seen.borrow().planned.is_empty());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

struct LegacyRouter;
impl EditorCommandRouter for LegacyRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
}

#[gpui::test]
fn image_admission_unimplemented_hook_and_absent_router_keep_legacy_route(cx: &mut TestAppContext) {
    for router in [
        None,
        Some(Rc::new(LegacyRouter) as Rc<dyn EditorCommandRouter>),
    ] {
        for target in [Target::Caret, Target::Range] {
            let m = mount(cx, target, Decision::Allow, false, false);
            m.window
                .update(cx, |view, _, _| view.set_command_router(router.clone()))
                .unwrap();
            paste(&m, cx);
            assert!(m.seen.borrow().admitted.is_empty());
            assert_eq!(
                m.seen.borrow().imports,
                usize::from(matches!(target, Target::Caret))
            );
        }
    }
}

struct FailedImport(Rc<RefCell<Seen>>);
impl AssetService for FailedImport {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(&self, _: AssetFormat, _: &[u8]) -> Result<ImageAttrs, AssetError> {
        self.0.borrow_mut().imports += 1;
        Err(AssetError::PermissionDenied)
    }
}

#[gpui::test]
fn image_admission_missing_or_failed_service_cannot_publish(cx: &mut TestAppContext) {
    for missing in [true, false] {
        let m = mount(cx, Target::All, Decision::Allow, false, false);
        m.window
            .update(cx, |view, _, _| {
                view.asset_service = if missing {
                    None
                } else {
                    Some(Rc::new(FailedImport(m.seen.clone())))
                };
            })
            .unwrap();
        let before = m.session.borrow().document().clone();
        let selection = m.session.borrow().selection();
        paste(&m, cx);
        assert_eq!(m.seen.borrow().admitted.len(), 1);
        assert_eq!(m.seen.borrow().imports, usize::from(!missing));
        assert!(m.seen.borrow().planned.is_empty());
        assert_eq!(m.session.borrow().document().store(), before.store());
        assert_eq!(m.session.borrow().selection(), selection);
        assert_eq!(m.session.borrow().history_depths(), (0, 0));
    }
}
