//! §18.2's five trace targets, turned into timeline events.
//!
//! # Why not `testutil::Capture`
//!
//! slither ships a capture fixture (`src/testutil/capture.rs`) that does
//! most of this, and it was the obvious choice. It is not used here for
//! one reason: a `CapturedEvent` carries `target`, `level`, `message` and
//! `fields`, and **no timestamp**. Draining it at await boundaries would
//! stamp every §18.2 event with the drain instant rather than the emission
//! instant, and would order all of them after whatever the scenario and
//! the wire logged in between — on a timeline whose entire job is showing
//! when things happened. This subscriber stamps
//! `tokio::time::Instant::now()` inside `event()` and appends to the same
//! log as everything else, so ordering and time are both real.
//!
//! (Written down so this is not later "simplified" back to `Capture`
//! without knowing what it costs.)
//!
//! # Scoped, not global
//!
//! `tracing::subscriber::set_default` — thread-local, like `Capture` — and
//! `register_callsite` answers `sometimes` rather than caching an
//! `Interest`, for the reason `capture.rs` documents: a process-wide
//! cached verdict would decide what this subscriber sees for reasons
//! outside the run that installed it.
//!
//! # The targets
//!
//! `slither::policy`, `slither::io`, `slither::frames`, `slither::roam`,
//! `slither::replay` — counted from `src/`, not from memory. The demo
//! forwards all five and lets the page decide what to show; the one that
//! earns its place in the MVP is `slither::replay`, whose single site
//! (`src/core/connection/session.rs:612`) is *"a received packet was
//! rejected by the replay window"* — the payoff of the duplication
//! scenario, available as data with no new slither code.

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

use crate::log::{Log, jstr};

/// A `tracing` subscriber that writes slither's events into the demo's
/// timeline.
pub struct TraceToLog {
    log: Log,
}

impl TraceToLog {
    /// Install for the rest of this thread, until the guard is dropped.
    pub fn install(log: Log) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(TraceToLog { log })
    }
}

impl Subscriber for TraceToLog {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // Only slither's own §18.2 targets. Anything tokio or a
        // dependency emits is noise on a protocol timeline.
        metadata.target().starts_with("slither::")
    }

    fn event(&self, event: &Event<'_>) {
        let meta = event.metadata();
        let mut visitor = Fields::default();
        event.record(&mut visitor);
        let fields = visitor
            .pairs
            .iter()
            .filter(|(k, _)| k != "message")
            .map(|(k, v)| format!("{}:{}", jstr(k), jstr(v)))
            .collect::<Vec<_>>()
            .join(",");
        self.log.emit(
            "trace",
            &format!(
                r#""target":{},"level":"{}","message":{},"fields":{{{fields}}}"#,
                jstr(meta.target()),
                meta.level(),
                jstr(&visitor.message),
            ),
        );
    }

    // Spans are not part of §18.2's contract and slither emits events, not
    // spans. These are the no-ops the trait requires.
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// Renders every reported field to a `String`.
///
/// `record_debug` is the catch-all and also carries `%x`: `tracing`'s
/// `DisplayValue` reports through it with a `Debug` impl that forwards to
/// `Display`, so `%addr` arrives already rendered as `10.0.0.2:4002`
/// rather than quoted.
#[derive(Default)]
struct Fields {
    message: String,
    pairs: Vec<(String, String)>,
}

impl Fields {
    fn put(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = value.clone();
        }
        self.pairs.push((field.name().to_owned(), value));
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value.to_owned());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, value.to_string());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, value.to_string());
    }
}
