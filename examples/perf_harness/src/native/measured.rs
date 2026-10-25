//! Wrap a subtree's request-layout through paint, including Taffy between calls.
use super::*;
pub(super) struct Measured {
    pub inner: AnyElement,
    pub host: WeakEntity<Host>,
    pub metrics: Rc<RefCell<Metrics>>,
    pub frame: usize,
    pub phase: &'static str,
    pub start: Option<Instant>,
}
impl IntoElement for Measured {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Measured {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.start = Some(Instant::now());
        let id = self.inner.request_layout(window, cx);
        self.record("request_layout", self.start.unwrap().elapsed());
        (id, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let start = Instant::now();
        self.inner.prepaint(window, cx);
        self.record("prepaint", start.elapsed());
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let start = Instant::now();
        self.inner.paint(window, cx);
        self.record("paint", start.elapsed());
        self.record("subtree_pipeline_cpu", self.start.unwrap().elapsed());
        if self.metrics.borrow().last_frame == Some(self.frame) {
            return;
        }
        self.metrics.borrow_mut().last_frame = Some(self.frame);
        let host = self.host.clone();
        let frame = self.frame;
        window.on_next_frame(move |window, cx| {
            host.update(cx, |host, cx| {
                if host.frame == frame {
                    host.advance(window, cx)
                }
            })
            .expect("measurement host remains mounted");
        });
    }
}
impl Measured {
    fn record(&self, stage: &str, time: std::time::Duration) {
        if self.phase != "warmup"
            && self.phase != "release"
            && self.metrics.borrow().last_frame != Some(self.frame)
        {
            self.metrics
                .borrow_mut()
                .times
                .push(&format!("{}_{stage}", self.phase), time.as_secs_f64() * 1e6);
        }
    }
}
