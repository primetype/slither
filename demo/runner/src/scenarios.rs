//! The scenarios. One invocation runs exactly one of these.
//!
//! Each is a plain application: it dials, exchanges data on a stream and
//! closes. Nothing here reaches past slither's public API, which is the
//! point — the page is showing what a consumer sees, not a rigged harness.

use std::time::Duration;

use slither::testutil::TestConnection;

use crate::log::{Log, jstr};
use crate::sim::{SCENARIO_CEILING, Sim, read_to_fin};
use crate::wire::Faults;

/// Everything argv can set.
pub struct Params {
    pub scenario: String,
    pub seed: u64,
    pub loss: f64,
    pub duplicate: f64,
    pub base_delay: Duration,
    pub jitter: Duration,
    pub message: String,
}

/// Run `params.scenario`, or report that there is no such scenario.
pub async fn run(params: &Params, log: &Log) -> Result<(), String> {
    let faults = Faults {
        loss: params.loss,
        duplicate: params.duplicate,
        base_delay: params.base_delay,
        jitter: params.jitter,
    };
    let sim = Sim::new(params.seed, faults, log.clone());
    let body = async {
        match params.scenario.as_str() {
            "clean" => clean(&sim, params).await,
            "lossy" => lossy(&sim, params).await,
            "duplicate" => duplicate(&sim, params).await,
            "partition" => partition(&sim, params).await,
            other => Err(format!("no such scenario: {other}")),
        }
    };
    match tokio::time::timeout(SCENARIO_CEILING, body).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "the scenario made no progress within {} s of virtual time",
            SCENARIO_CEILING.as_secs()
        )),
    }
}

/// **Scenario 1 — clean run.** Handshake, one message each way on one
/// bidirectional stream, clean close. The DH ladder and §6.2's four rungs
/// are the story; nothing is lost.
async fn clean(sim: &Sim, params: &Params) -> Result<(), String> {
    let (a, b) = sim.establish().await?;
    note(&sim.log, "the handshake is done; §6.1's DH cost is final");
    exchange(sim, &a, &b, &params.message).await?;
    close(sim, &a, &b).await
}

/// **Scenario 2 — lossy handshake.** The same application over a lossy
/// wire. What the timeline shows is §5.5's retransmission: a fresh
/// ephemeral every `REKEY_TIMEOUT`, so the gaps on the axis are 5-second
/// steps in virtual time and milliseconds of the visitor's.
async fn lossy(sim: &Sim, params: &Params) -> Result<(), String> {
    let (a, b) = sim.establish().await?;
    note(
        &sim.log,
        "the handshake completed; each 5 s gap above is one REKEY_TIMEOUT \
         expiry retransmitting with a fresh ephemeral (§5.5)",
    );
    exchange(sim, &a, &b, &params.message).await?;
    close(sim, &a, &b).await
}

/// **Scenario 3 — duplication.** Every datagram may arrive twice. The
/// session takes no notice: the second copy fails §7.2's replay window and
/// is discarded, which slither reports on the `slither::replay` trace
/// target (§18.2) — the `trace` events on the timeline are that rejection,
/// not the demo's commentary.
async fn duplicate(sim: &Sim, params: &Params) -> Result<(), String> {
    let (a, b) = sim.establish().await?;
    note(
        &sim.log,
        "duplicates from here on are rejected by the replay window; watch the \
         slither::replay traces and note that no session state moves",
    );
    // Several exchanges, because one duplicate is an anecdote.
    for i in 0..3 {
        exchange(sim, &a, &b, &format!("{} #{i}", params.message)).await?;
    }
    close(sim, &a, &b).await
}

/// **Scenario 4 — partition.** Established, exchanging, then the path goes
/// away. Nothing is signalled, because there is nobody to signal: the
/// connection dies on §7.4's receive-driven liveness bound, in silence.
///
/// `a` keeps sending into the void deliberately. Liveness is receive-driven
/// and the death clock is armed by a *marking send* (§7.5), so an endpoint
/// that goes quiet at the same moment the path does is not the interesting
/// case — it is the one where nothing is owed.
async fn partition(sim: &Sim, params: &Params) -> Result<(), String> {
    let (a, b) = sim.establish().await?;
    exchange(sim, &a, &b, &params.message).await?;

    sim.a.wire.set_blackholed(true);
    sim.b.wire.set_blackholed(true);
    sim.log.emit(
        "fault",
        r#""what":"partition","detail":"both paths blackholed; sends still succeed""#,
    );

    // Arm the clock: something must be owed for the liveness bound to be
    // the thing that fires.
    let mut send = a.open_uni().await.map_err(|e| format!("open_uni: {e}"))?;
    let _ = send.write(b"into the void").await;
    note(
        &sim.log,
        "a is still sending; every datagram is accepted by the socket and \
         reaches nobody",
    );

    let started = sim.log.now_us();
    let cause = a.closed().await;
    let elapsed = sim.log.now_us() - started;
    sim.log.emit(
        "lost",
        &format!(
            r#""side":"a","cause":{},"silent_us":{elapsed}"#,
            jstr(&format!("{cause}"))
        ),
    );
    // `b` is in exactly the same position, from the other side.
    let cause_b = b.closed().await;
    sim.log.emit(
        "lost",
        &format!(r#""side":"b","cause":{}"#, jstr(&format!("{cause_b}"))),
    );
    Ok(())
}

/// One message each way on one bidirectional stream.
async fn exchange(
    sim: &Sim,
    a: &TestConnection,
    b: &TestConnection,
    message: &str,
) -> Result<(), String> {
    let reply = format!("ack: {message}");
    let dialler = async {
        let (mut send, mut recv) = a
            .open_bi()
            .await
            .map_err(|e| format!("open_bi: {e}"))?
            .split();
        write_all(&mut send, message.as_bytes()).await?;
        send.finish().await.map_err(|e| format!("finish: {e}"))?;
        sim.log.emit(
            "app",
            &format!(
                r#""side":"a","op":"sent","bytes":{},"text":{}"#,
                message.len(),
                jstr(message)
            ),
        );
        let got = read_to_fin(&mut recv).await?;
        sim.log.emit(
            "app",
            &format!(
                r#""side":"a","op":"received","bytes":{},"text":{}"#,
                got.len(),
                jstr(&String::from_utf8_lossy(&got))
            ),
        );
        Ok::<(), String>(())
    };
    let responder = async {
        let (mut send, mut recv) = b
            .accept_bi()
            .await
            .map_err(|e| format!("accept_bi: {e}"))?
            .split();
        let got = read_to_fin(&mut recv).await?;
        sim.log.emit(
            "app",
            &format!(
                r#""side":"b","op":"received","bytes":{},"text":{}"#,
                got.len(),
                jstr(&String::from_utf8_lossy(&got))
            ),
        );
        write_all(&mut send, reply.as_bytes()).await?;
        send.finish().await.map_err(|e| format!("finish: {e}"))?;
        sim.log.emit(
            "app",
            &format!(
                r#""side":"b","op":"sent","bytes":{},"text":{}"#,
                reply.len(),
                jstr(&reply)
            ),
        );
        Ok::<(), String>(())
    };
    let (l, r) = tokio::join!(dialler, responder);
    l?;
    r
}

/// `write` accepts what flow-control credit admits and may accept less
/// than the whole buffer, so the loop is the contract, not a convenience.
async fn write_all(
    send: &mut slither::testutil::TestSendStream,
    mut buf: &[u8],
) -> Result<(), String> {
    while !buf.is_empty() {
        let n = send.write(buf).await.map_err(|e| format!("write: {e}"))?;
        buf = &buf[n..];
    }
    Ok(())
}

/// `a` closes; `b` observes it as `PeerClosed`.
async fn close(sim: &Sim, a: &TestConnection, b: &TestConnection) -> Result<(), String> {
    a.close(0, b"demo over").await;
    sim.log.emit(
        "app",
        r#""side":"a","op":"close","code":0,"reason":"demo over""#,
    );
    let cause = b.closed().await;
    sim.log.emit(
        "lost",
        &format!(r#""side":"b","cause":{}"#, jstr(&format!("{cause}"))),
    );
    Ok(())
}

fn note(log: &Log, text: &str) {
    log.emit("note", &format!(r#""text":{}"#, jstr(text)));
}
