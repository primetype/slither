//! `slither-demo` — one scenario run, one JSON-line timeline on stdout.
//!
//! Built for `wasm32-wasip1` and executed inside a Web Worker by
//! `demo/web/worker.js`, which supplies argv through WASI `args_get` and
//! reads stdout through `fd_write`. It builds natively too, which is how
//! it is developed and tested; nothing in it is wasm-specific.
//!
//! # The browser cannot send UDP, and this does not pretend otherwise
//!
//! Both endpoints are in one process, talking over slither's own in-memory
//! datagram fabric (`testutil::Network`, SPEC.md §16.10). That is not a
//! workaround invented for the demo — it is how every one of slither's
//! flow tests runs, on the same paused clock, and it is the reason the
//! protocol is testable without a kernel at all.
//!
//! # Virtual time
//!
//! The runtime is built with `start_paused(true)`, so tokio advances the
//! clock to the next deadline whenever every task is idle. A 10-second
//! retransmission ladder therefore costs about ten milliseconds of wall
//! time, and — the part that matters for a browser — the runtime **never
//! parks on a real timer**, so the WASI shim's `poll_oneoff` (a busy-wait
//! spin loop in `browser_wasi_shim`) is never called.

mod log;
mod scenarios;
mod sim;
mod trace;
mod wire;

use std::time::Duration;

use crate::log::{Log, jstr};
use crate::scenarios::Params;

fn main() {
    let params = match parse(std::env::args().skip(1)) {
        Ok(p) => p,
        Err(e) => {
            // Emitted in the event format so the page can show it in the
            // same place it shows everything else.
            println!(
                r#"{{"seq":0,"t_us":0,"kind":"error","message":{}}}"#,
                jstr(&e)
            );
            std::process::exit(2);
        }
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        // The demo's fast-forward, and the reason `poll_oneoff` is never
        // reached. Not `slither::block_on`, which builds an unpaused
        // runtime — the shell it drives is identical either way.
        .start_paused(true)
        .build()
        .expect("a current-thread tokio runtime");

    // The shell is a `!Send` actor spawned with `spawn_local` (§16.3), so
    // everything runs inside a `LocalSet`.
    let local = tokio::task::LocalSet::new();
    let wall = std::time::Instant::now();
    // Everything, `end` included, is emitted **inside** `block_on`.
    // Outside it there is no runtime, so `tokio::time::Instant::now()`
    // falls back to the real clock and the paused origin becomes a wall
    // reading: the first version of this emitted `end` after the runtime
    // returned and stamped a 56 ms virtual timestamp on a run whose every
    // other event was at 0. The number looked plausible, which is what
    // made it worth pinning here.
    let failed = local.block_on(&runtime, async move {
        let log = Log::start();
        // Installed inside the `LocalSet`'s thread: `set_default` is
        // thread-local, and the driver emits on this thread.
        let _guard = trace::TraceToLog::install(log.clone());
        log.emit(
            "run",
            &format!(
                r#""scenario":{},"seed":{},"loss":{},"duplicate":{},"delay_us":{},"jitter_us":{},"message":{}"#,
                jstr(&params.scenario),
                params.seed,
                params.loss,
                params.duplicate,
                params.base_delay.as_micros(),
                params.jitter.as_micros(),
                jstr(&params.message),
            ),
        );
        let result = scenarios::run(&params, &log).await;
        let c = log.counters();
        let wall_us = wall.elapsed().as_micros();
        let totals = format!(
            r#""wall_us":{wall_us},"sent":{},"delivered":{},"lost":{},"duplicated":{},"blackholed":{}"#,
            c.sent, c.delivered, c.lost, c.duplicated, c.blackholed
        );
        match result {
            Ok(()) => {
                log.emit("end", &format!(r#""ok":true,{totals}"#));
                false
            }
            Err(e) => {
                log.emit(
                    "end",
                    &format!(r#""ok":false,"error":{},{totals}"#, jstr(&e)),
                );
                true
            }
        }
    });

    if failed {
        std::process::exit(1);
    }
}

/// `--key value` pairs, all optional.
///
/// Hand-rolled rather than `clap`: six flags, and every kilobyte of this
/// binary is downloaded by a visitor.
fn parse(args: impl Iterator<Item = String>) -> Result<Params, String> {
    let mut p = Params {
        scenario: "clean".to_owned(),
        seed: 0xC0FFEE,
        loss: 0.0,
        duplicate: 0.0,
        base_delay: Duration::ZERO,
        jitter: Duration::ZERO,
        message: "hello from WebAssembly".to_owned(),
    };
    let args: Vec<String> = args.collect();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} needs a value"))?;
        match key {
            "--scenario" => p.scenario = value.clone(),
            "--seed" => p.seed = value.parse().map_err(|_| format!("bad seed: {value}"))?,
            "--loss" => p.loss = unit(value, "loss")?,
            "--duplicate" => p.duplicate = unit(value, "duplicate")?,
            "--delay-ms" => p.base_delay = millis(value, "delay-ms")?,
            "--jitter-ms" => p.jitter = millis(value, "jitter-ms")?,
            "--message" => {
                // Capped because it rides argv, and because a demo that
                // lets a URL parameter grow without bound is a demo with
                // a denial-of-service knob on it.
                p.message = value.chars().take(120).collect();
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 2;
    }
    Ok(p)
}

/// A probability. Rejected outside `[0, 1]` rather than clamped: a clamp
/// would silently accept `--loss 50` as "50%" when it means "always".
fn unit(value: &str, what: &str) -> Result<f64, String> {
    let v: f64 = value
        .parse()
        .map_err(|_| format!("bad {what}: {value} is not a number"))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("bad {what}: {v} is outside [0, 1]"));
    }
    Ok(v)
}

fn millis(value: &str, what: &str) -> Result<Duration, String> {
    let v: u64 = value
        .parse()
        .map_err(|_| format!("bad {what}: {value} is not a whole number of ms"))?;
    if v > 60_000 {
        return Err(format!("bad {what}: {v} ms is longer than a minute"));
    }
    Ok(Duration::from_millis(v))
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn defaults_hold_and_flags_override() {
        let p = parse(args(&[]).into_iter()).expect("no args is valid");
        assert_eq!(p.scenario, "clean");
        assert_eq!(p.loss, 0.0);
        let p = parse(args(&["--scenario", "lossy", "--loss", "0.25"]).into_iter())
            .expect("valid flags");
        assert_eq!(p.scenario, "lossy");
        assert_eq!(p.loss, 0.25);
    }

    #[test]
    fn an_out_of_range_probability_is_rejected_rather_than_clamped() {
        // The degenerate implementation here is a clamp, which accepts
        // both of these and silently means something else.
        assert!(parse(args(&["--loss", "50"]).into_iter()).is_err());
        assert!(parse(args(&["--duplicate", "-0.5"]).into_iter()).is_err());
        // The boundaries themselves are valid.
        assert!(parse(args(&["--loss", "0"]).into_iter()).is_ok());
        assert!(parse(args(&["--loss", "1"]).into_iter()).is_ok());
    }

    #[test]
    fn a_missing_value_or_unknown_flag_is_an_error() {
        assert!(parse(args(&["--loss"]).into_iter()).is_err());
        assert!(parse(args(&["--nonsense", "1"]).into_iter()).is_err());
    }

    #[test]
    fn the_message_is_length_capped() {
        let long = "x".repeat(500);
        let p = parse(args(&["--message", &long]).into_iter()).expect("valid");
        assert_eq!(p.message.chars().count(), 120);
    }
}
