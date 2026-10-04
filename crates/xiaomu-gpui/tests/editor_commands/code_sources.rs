//! Source-aware hooks coexist with old exhaustive routers and transport rules.
use super::*;
use xiaomu_gpui::{
    editor::bind_primary_modifier_enter_keys,
    editor_commands::{CodePasteSource, EnterSource, PrimaryModifierEnter},
};
use xiaomu_runtime::session::{IntentDisposition, SessionContext, SessionPolicy};

#[path = "code_sources/arrow_down.rs"]
mod arrow_down;
#[path = "code_sources/enter.rs"]
mod enter;
#[path = "code_sources/paste.rs"]
mod paste;
#[path = "code_sources/rejection.rs"]
mod rejection;
#[path = "code_sources/slices.rs"]
mod slices;

#[derive(Clone, Copy)]
enum CodeDecision {
    Default,
    Raw,
    NoChange,
    Reject,
    RejectedCandidate,
}

#[derive(Debug, PartialEq, Eq)]
enum CodeGesture {
    Enter(EnterSource),
    Paste(String, CodePasteSource),
}

struct CodeObserved {
    gesture: CodeGesture,
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
}

struct CodeRouter {
    decision: CodeDecision,
    observed: RefCell<Vec<CodeObserved>>,
    ordinary: Cell<usize>,
}

impl CodeRouter {
    fn new(decision: CodeDecision) -> Rc<Self> {
        Rc::new(Self {
            decision,
            observed: RefCell::new(Vec::new()),
            ordinary: Cell::new(0),
        })
    }

    fn decide(
        &self,
        context: EditorCommandContext<'_>,
        gesture: CodeGesture,
        intent: EditIntent,
    ) -> Result<CommandRoute, PolicyError> {
        // Recording is test-only instrumentation, never session mutation.
        self.observed.borrow_mut().push(CodeObserved {
            gesture,
            document: context.document().clone(),
            selection: context.selection(),
            marks: context.stored_marks().cloned(),
        });
        match self.decision {
            CodeDecision::Default => Ok(CommandRoute::Default),
            CodeDecision::NoChange => Ok(CommandRoute::NoChange),
            CodeDecision::Reject => Err(PolicyError::new("code route rejected")),
            CodeDecision::Raw => Ok(CommandRoute::Intent(intent)),
            CodeDecision::RejectedCandidate => Ok(CommandRoute::Intent(EditIntent::PasteText {
                text: "!".into(),
            })),
        }
    }
}

impl EditorCommandRouter for CodeRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        self.ordinary.set(self.ordinary.get() + 1);
        Ok(CommandRoute::Default)
    }

    fn route_enter(
        &self,
        context: EditorCommandContext<'_>,
        source: EnterSource,
    ) -> Result<CommandRoute, PolicyError> {
        let marker = match source {
            EnterSource::Plain => "plain",
            EnterSource::Shift => "shift",
            EnterSource::PrimaryModifier => "primary",
        };
        self.decide(
            context,
            CodeGesture::Enter(source),
            EditIntent::PasteText {
                text: marker.into(),
            },
        )
    }

    fn route_code_paste(
        &self,
        context: EditorCommandContext<'_>,
        raw: &str,
        source: CodePasteSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.decide(
            context,
            CodeGesture::Paste(raw.into(), source),
            EditIntent::PasteText { text: raw.into() },
        )
    }
}

#[derive(Default)]
struct CodePolicy {
    pasted: Rc<RefCell<Vec<String>>>,
}

impl SessionPolicy for CodePolicy {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::PasteText { text } = intent {
            self.pasted.borrow_mut().push(text.clone());
        }
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .any(|run| run.text().as_str().contains('!'))
        {
            return Err(PolicyError::new("candidate rejected"));
        }
        Ok(())
    }
}

fn raw_slice(raw: &str, closed: bool) -> ClipboardSlice {
    let (document, node) = single(NodeKind::Paragraph, raw);
    let selection = if closed {
        DocumentSelection::all(&document)
    } else {
        DocumentSelection::new(point(&document, node, 0), point(&document, node, raw.len()))
    };
    let slice = DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert_eq!(slice.is_closed(), closed);
    assert_eq!(slice.plain_text(), raw);
    slice
}

fn write_slice(slice: &ClipboardSlice, cx: &mut TestAppContext) {
    let metadata = xiaomu_runtime::clipboard::encode_metadata(slice).unwrap();
    assert_eq!(
        xiaomu_runtime::clipboard::decode_metadata(slice.plain_text(), &metadata),
        Some(slice.clone())
    );
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
            slice.plain_text().into(),
            metadata,
        ));
    });
}
