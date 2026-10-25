//! Native window CPU pipeline measurement; not GPU completion or input latency.
#![forbid(unsafe_code)]
mod measured;
use gpui::{prelude::*, *};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc, time::Instant};
use xiaomu_gpui::{document_view::DocumentView, editor::bind_default_editor_keys};
use xiaomu_perf_harness::{
    fixtures::{self, Fixture},
    metrics::{Timings, resident_bytes},
};
use xiaomu_runtime::session::EditIntent;

struct Host {
    fixture: Fixture,
    views: Vec<Entity<DocumentView>>,
    released: Vec<WeakEntity<DocumentView>>,
    metrics: Rc<RefCell<Metrics>>,
    frame: usize,
    samples: usize,
    counts: Vec<usize>,
    finishing: bool,
    idle_ms: u64,
}
#[derive(Default)]
struct Metrics {
    times: Timings,
    memory: Vec<Value>,
    last_frame: Option<usize>,
    viewport: [f32; 2],
    scenario: String,
    scale: f32,
    redraw_wakeups: usize,
}
impl Host {
    fn phase(&self) -> &'static str {
        match self.frame {
            0 => "initial",
            n if n <= 5 => "warmup",
            n if n <= 5 + self.samples => "redraw",
            n if n <= 5 + 2 * self.samples => "insert",
            _ => "release",
        }
    }
    fn memory(&self, stage: &str) {
        let depths: Vec<_> = self.views.iter().map(|v| v.entity_id().as_u64()).collect();
        self.metrics
            .borrow_mut()
            .memory
            .push(json!({"stage":stage,"rss_bytes":resident_bytes(),"view_ids":depths}));
    }
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finishing {
            return;
        }
        if self.frame == 0 {
            self.memory("mounted");
        }
        eprintln!("p6 frame={} phase={}", self.frame, self.phase());
        self.frame += 1;
        if self.frame == 6 + 2 * self.samples {
            for (index, view) in self.views.iter().enumerate() {
                let session = view.read(cx).session().borrow();
                assert_eq!(
                    fixtures::leaf_text(session.document(), self.fixture.target),
                    format!(
                        "{}{}",
                        "界".repeat(self.counts[index]),
                        fixtures::leaf_text(&self.fixture.document, self.fixture.target)
                    )
                );
                assert_eq!(
                    session.history_depths(),
                    (usize::from(self.counts[index] > 0), 0)
                );
            }
            self.memory("edited");
            self.released = self.views.iter().map(Entity::downgrade).collect();
            self.views.clear();
        } else if self.frame > 9 + 2 * self.samples {
            assert!(
                self.released.iter().all(|view| view.upgrade().is_none()),
                "view owners leaked after release frames"
            );
            self.memory("views_released");
            self.finishing = true;
            let idle_ms = self.idle_ms;
            cx.spawn(async move |host, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(idle_ms))
                    .await;
                host.update(cx, |host, cx| {
                    host.memory("idle_after_release");
                    host.report();
                    cx.quit();
                })
                .expect("measurement host remains mounted for idle checkpoint");
            })
            .detach();
            return;
        } else if self.phase() == "insert" {
            let index = (self.frame - 6 - self.samples) % self.views.len();
            let start = Instant::now();
            self.views[index].update(cx, |view, cx| {
                view.apply_edit_intent(EditIntent::InsertText { text: "界".into() }, window, cx)
            });
            self.metrics
                .borrow_mut()
                .times
                .push("insert_command_cpu", start.elapsed().as_secs_f64() * 1e6);
            self.counts[index] += 1;
        }
        // Explicitly request a full document redraw; this is not an idle frame.
        for view in &self.views {
            view.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }
    fn report(&self) {
        let state = self.metrics.borrow();
        println!(
            "{}",
            json!({"schema":1,"kind":"native","scenario":state.scenario,"samples":self.samples,
            "window_bounds_logical_px":[1280,800], "viewport_logical_px":state.viewport,
            "editors":self.fixture.editors, "nodes_per_document":self.fixture.document.node_count(),
            "scale_factor":state.scale,"font_px":16,"timings":state.times.json(),
            "memory":state.memory,"released_views_verified":true,"idle_ms":self.idle_ms,"redraw_wakeups":state.redraw_wakeups})
        );
        drop(state);
    }
}
impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.metrics.borrow_mut().scale = window.scale_factor();
        self.metrics.borrow_mut().viewport = [
            window.viewport_size().width.into(),
            window.viewport_size().height.into(),
        ];
        let width = 1280.0 / self.fixture.editors as f32;
        let inner = div()
            .size_full()
            .bg(white())
            .text_color(black())
            .text_size(px(16.0))
            .flex()
            .children(self.views.iter().map(|view| {
                div()
                    .w(px(width))
                    .h_full()
                    .overflow_hidden()
                    .child(view.clone())
            }))
            .into_any_element();
        measured::Measured {
            inner,
            host: cx.entity().downgrade(),
            metrics: self.metrics.clone(),
            frame: self.frame,
            phase: self.phase(),
            start: None,
        }
    }
}
fn main() {
    if cfg!(debug_assertions) {
        eprintln!("performance measurements require a release build");
        std::process::exit(2);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let name = args
        .first()
        .map(String::as_str)
        .unwrap_or("long-1k")
        .to_owned();
    let samples: usize = args.get(1).map(|v| v.parse().unwrap()).unwrap_or(30);
    assert!((1..=1000).contains(&samples));
    let idle_ms: u64 = std::env::var("XIAOMU_PERF_IDLE_MS")
        .ok()
        .map(|v| v.parse().expect("idle milliseconds"))
        .unwrap_or(1000);
    assert!((1000..=30_000).contains(&idle_ms));
    let metrics = Rc::new(RefCell::new(Metrics::default()));
    metrics
        .borrow_mut()
        .memory
        .push(json!({"stage":"process_start","rss_bytes":resident_bytes()}));
    metrics.borrow_mut().scenario = name.clone();
    let state = metrics.clone();
    let scenario = name.clone();
    Application::new().run(move |cx| {
        bind_default_editor_keys(cx);
        let fixture = fixtures::build(&scenario);
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Xiaomu P6 measurement".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let views = fixture
                            .sessions()
                            .into_iter()
                            .map(|session| {
                                cx.new(|_| {
                                    let mut view =
                                        DocumentView::new(Rc::new(RefCell::new(session)));
                                    view.set_measured_table_layout(true);
                                    view
                                })
                            })
                            .collect();
                        Host {
                            counts: vec![0; fixture.editors],
                            finishing: false,
                            idle_ms,
                            fixture,
                            views,
                            released: vec![],
                            metrics: state,
                            frame: 0,
                            samples,
                        }
                    })
                },
            )
            .expect("open native measurement window");
        handle
            .update(cx, |_, window, _| window.activate_window())
            .expect("activate measurement window");
        // A native host may coalesce redraw notifications. Recover a stalled
        // requested frame without advancing the scenario or timing the wait.
        cx.spawn(async move |cx| {
            let mut observed = usize::MAX;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                if handle
                    .update(cx, |host, window, _| {
                        if !host.finishing && host.frame == observed {
                            host.metrics.borrow_mut().redraw_wakeups += 1;
                            window.refresh();
                        }
                        observed = host.frame;
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.activate(true);
    });
}
