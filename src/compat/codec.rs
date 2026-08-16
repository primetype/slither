//! §16.11's framing face — `feature = "codec"`.
//!
//! Two constructors and nothing else. `PLAN.md` §3.3: the codec surface
//! *"falls out of §3.1 for free; the constructors exist only to remove a
//! `use`"*, and the result is *"identical to
//! `Framed::new(conn.open_bi().await?, codec)`"*.
//!
//! That it works at all is entirely `compat::io`'s doing:
//! [`tokio_util::codec::Framed`] needs `AsyncRead + AsyncWrite` on its
//! transport, which [`BiStream`] has because §3 of the contract gives it
//! both. That is the whole of the dependency — which is why the `codec`
//! feature implies `sink` in the manifest but the *type* needs nothing from
//! `compat::stream`.
//!
//! # Deliberately absent
//!
//! There is **no `framed_uni` / `accept_framed_uni`**. `FramedWrite`/
//! `FramedRead` over the two half-handles are not in `PLAN.md` §3.3, and a
//! consumer writes `FramedWrite::new(conn.open_uni().await?, codec)` in one
//! line.

use tokio_util::codec::Framed;

use crate::error::ConnectionLost;
use crate::packet::Handshake;
use crate::shell::{BiStream, Connection};

impl<S: Handshake> Connection<S> {
    /// `Framed::new(self.open_bi().await?, codec)`.
    ///
    /// Opens one bidirectional stream and wraps it in `codec`. Waits for
    /// §10.4's stream allowance exactly as [`open_bi`](Self::open_bi) does,
    /// and is cancel-safe for the same reason: a dropped future has opened
    /// no stream.
    ///
    /// `C` carries **no bound** here — `Framed::new` requires none. The
    /// [`Encoder`](tokio_util::codec::Encoder)/[`Decoder`](tokio_util::codec::Decoder)
    /// bounds appear on `Framed`'s own `Sink`/`Stream` impls, where you meet
    /// them at the call that sends or receives.
    pub async fn framed_bi<C>(&self, codec: C) -> Result<Framed<BiStream<S>, C>, ConnectionLost> {
        Ok(Framed::new(self.open_bi().await?, codec))
    }

    /// `Framed::new(self.accept_bi().await?, codec)`.
    ///
    /// Claims the next peer-opened bidirectional stream — FIFO, in open
    /// order — and wraps it in `codec`. [`accept_bi`](Self::accept_bi)'s
    /// terms in every respect, including ruling 128's drain: a stream the
    /// peer opened before the connection died is still handed over.
    pub async fn accept_framed_bi<C>(
        &self,
        codec: C,
    ) -> Result<Framed<BiStream<S>, C>, ConnectionLost> {
        Ok(Framed::new(self.accept_bi().await?, codec))
    }
}
