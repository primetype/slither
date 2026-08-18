//! Capturing `tracing` events, so §18.2's operator contract is testable.
//!
//! # Why this exists
//!
//! §18.2's five trace targets are **contract** — *"renaming or dropping one
//! is a protocol revision"* — and two obligations are stated as MUSTs with
//! no other surface at all: ruling 49's failing-`send_to` trace (§16.3,
//! S25) and ruling 59's message-mode overflow reset (§9.8, S30). Neither
//! resolves a verb, produces a `Notification` or adds an error variant, on
//! purpose: §18.1's taxonomy is closed. The trace **is** the whole
//! obligation, so a suite with no way to observe an event cannot discharge
//! either of them, and `tests/spec_shell.rs`'s gap **G7** records exactly
//! that.
//!
//! [`Capture`] closes it: install one, drive the protocol, and assert on
//! the events by target, level and field.
//!
//! ```no_run
//! # use slither::testutil::{Capture, local};
//! # async fn body() {
//! local(async {
//!     let capture = Capture::install();
//!     // … drive two endpoints …
//!     let traced = capture.with_target("slither::io");
//!     assert_eq!(traced[0].field("verb"), Some("send_to"));
//! })
//! .await;
//! # }
//! ```
//!
//! # Why it is hand-rolled
//!
//! `tracing-subscriber` is not a dependency and is not added for this.
//! What is needed here is *"record every event as plain data"*, which is
//! one `Subscriber` impl and one `Visit` impl over the `tracing` API
//! slither already depends on — no filtering language, no layer stack, no
//! formatting, none of which a field assertion wants. Keeping it in-tree
//! also keeps the captured event a **slither** type: a test asserts on
//! `&str` fields and never names a `tracing` type, so the fixture cannot
//! drag `tracing` into slither's public API the way re-exporting `Level` or
//! `DefaultGuard` would.
//!
//! # Thread-local by construction, and that is the isolation
//!
//! [`Capture::install`] sets a **scoped** default dispatcher
//! (`tracing::subscriber::set_default`), not a global one. Two consequences
//! matter for flow tests:
//!
//! * The whole shell is a `!Send` actor driven on one thread inside a
//!   `LocalSet` (§16.3), so every event a driver emits is emitted on the
//!   thread that installed the capture, and is seen.
//! * Tests in one binary run on different threads, so two `Capture`s never
//!   see each other's events — where a global default would be a single
//!   process-wide slot that the first test to claim it wins.
//!
//! Install it **inside** [`local`](super::local), therefore, not around it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::{DefaultGuard, Interest};
use tracing::{Event, Metadata, Subscriber};

/// One `tracing` event, flattened to plain data.
///
/// Every field value is rendered to a `String` at capture time — with
/// `Display` for `%x`, `Debug` for `?x`, and the plain rendering for the
/// primitives — because a `tracing` field value does not outlive the call
/// that reported it, and because the assertion a test wants to write is a
/// string comparison either way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedEvent {
    /// The §18.2 target, e.g. `"slither::io"`.
    pub target: String,
    /// `"TRACE"`, `"DEBUG"`, `"INFO"`, `"WARN"` or `"ERROR"`.
    ///
    /// A `String` rather than `tracing::Level` deliberately: `testutil` is
    /// public surface, and this fixture does not put a `tracing` type in
    /// slither's public API.
    pub level: String,
    /// The event's message — the `tracing` field named `message`, which is
    /// where the format string lands. Empty if the event had none.
    pub message: String,
    /// Every field in report order, `message` included.
    pub fields: Vec<(String, String)>,
}

impl CapturedEvent {
    /// The value reported for `name`, or `None` if the event has no such
    /// field.
    ///
    /// Distinguishing *absent* from *empty* is the point: §18.2 names the
    /// payload a target carries, so "the field is not there at all" is the
    /// failure a conformance test is looking for.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the message contains `needle`.
    ///
    /// For the one §18.2 payload that is prose rather than a field: ruling
    /// 59's *"the mode conflict"*.
    pub fn message_contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}

/// A scoped `tracing` subscriber that records every event for assertion.
///
/// Alive for as long as the value is: dropping it restores whatever
/// dispatcher was in place before. It is `!Send`, like everything else on
/// the actor path.
#[derive(Debug)]
pub struct Capture {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
    // Dropped last, restoring the previous default dispatcher. Held, never
    // read.
    _guard: DefaultGuard,
}

impl Capture {
    /// Record every event emitted on **this thread** until the returned
    /// value is dropped.
    ///
    /// `tracing`'s scoped default is thread-local, so a capture installed
    /// outside a `LocalSet`'s thread sees nothing — install it inside
    /// [`local`](super::local).
    pub fn install() -> Capture {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = CaptureSubscriber {
            events: Arc::clone(&events),
            next_span: AtomicU64::new(1),
        };
        let guard = tracing::subscriber::set_default(subscriber);
        Capture {
            events,
            _guard: guard,
        }
    }

    /// Everything captured so far, in emission order.
    pub fn events(&self) -> Vec<CapturedEvent> {
        self.lock().clone()
    }

    /// Everything captured so far on one §18.2 target, in emission order.
    pub fn with_target(&self, target: &str) -> Vec<CapturedEvent> {
        self.lock()
            .iter()
            .filter(|e| e.target == target)
            .cloned()
            .collect()
    }

    /// Drop everything captured so far.
    ///
    /// The instrument for *"ignore the handshake, assert on what happens
    /// next"*: a flow test establishes a connection, clears, and then
    /// provokes the condition under test.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// How many events have been captured.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing has been captured.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<CapturedEvent>> {
        self.events.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The `Subscriber` behind [`Capture`].
///
/// `Send + Sync` because `tracing::Dispatch::new` requires it — which says
/// nothing about the actor path: this type is the test's, never slither's.
#[derive(Debug)]
struct CaptureSubscriber {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
    next_span: AtomicU64,
}

impl Subscriber for CaptureSubscriber {
    /// `sometimes`, never `always`/`never`.
    ///
    /// `tracing` caches a callsite's `Interest` **process-wide**, while this
    /// subscriber is only ever a *scoped* default. A cached `never` — or a
    /// cached `always` from some other test's subscriber — would decide
    /// whether an event reaches this capture for reasons outside the test
    /// that installed it. `sometimes` asks [`enabled`](Self::enabled) every
    /// time, which is the only answer that is correct under a thread-local
    /// dispatcher.
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        // slither emits events only — no `span!`, no `#[instrument]`. This
        // exists to satisfy the trait, and hands out distinct ids so a
        // future span would not be mistaken for the root.
        Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let meta = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor
            .fields
            .iter()
            .find(|(k, _)| k == "message")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let captured = CapturedEvent {
            target: meta.target().to_owned(),
            level: meta.level().as_str().to_uppercase(),
            message,
            fields: visitor.fields,
        };
        if let Ok(mut events) = self.events.lock() {
            events.push(captured);
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// Renders every reported field to a `String`.
#[derive(Default)]
struct FieldVisitor {
    fields: Vec<(String, String)>,
}

impl FieldVisitor {
    fn push(&mut self, field: &Field, value: String) {
        self.fields.push((field.name().to_owned(), value));
    }
}

impl Visit for FieldVisitor {
    /// The catch-all, and the one that carries `%x` as well as `?x`:
    /// `tracing`'s `DisplayValue` reports through `record_debug` with a
    /// `Debug` impl that forwards to `Display`, so `to = %addr` arrives here
    /// already rendered as `10.0.0.2:4002` rather than quoted.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, value.to_owned());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field, value.to_string());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.push(field, value.to_string());
    }
}
