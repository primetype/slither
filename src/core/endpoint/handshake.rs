//! §5.5–5.7's initiator driving, and §5.6's responder msg2 — the two
//! places this core actually steps a hiss state machine.
//!
//! # Framing
//!
//! Both packets are `header ‖ Noise message ‖ mac1`, and mac1's preimage is
//! §4.1's "all packet bytes preceding the tag" — taken as the buffer built
//! so far rather than re-derived, so there is no second encoding to
//! disagree with the first.
//!
//! **mac1 is keyed on the recipient's static** (§4.3). Outbound, that is
//! the peer's; inbound, it is ours. The two are one line apart and produce
//! no type error when swapped, which is why each key derivation below names
//! whose key it is.
//!
//! # One attempt builder, used by the first send and every retransmit
//!
//! §5.5: *"Every retransmit is a completely fresh initiation — new
//! ephemeral, new random index, new strictly-greater timestamp."* Writing
//! the first send and the retransmit as one function is what makes that
//! true by construction; two functions would drift, and the drift would be
//! invisible until an interop test.

use packtool::Packet;

use crate::packet::{InitHeader, Mac1Key, RespHeader};

/// Frame an outbound handshake initiation. §3.2, §4.1.
///
/// `peer_mac1` is derived from the **recipient's** static — the peer we are
/// dialling.
pub(crate) fn frame_init(sender_index: u32, msg1: &[u8], peer_mac1: &Mac1Key) -> Vec<u8> {
    let header = Packet::pack(&InitHeader::new(sender_index));
    let header: &[u8] = header.as_ref();

    let mut out = Vec::with_capacity(header.len() + msg1.len() + crate::constants::MAC1_LEN);
    out.extend_from_slice(header);
    out.extend_from_slice(msg1);
    let tag = peer_mac1.tag(&out);
    out.extend_from_slice(&tag);
    out
}

/// Frame an outbound handshake response. §3.3, §4.1.
///
/// Note the header's order — **ours, then theirs** — and that `peer_mac1`
/// is again the recipient's key: here, the initiator's static, which the
/// `ss` has just proven they hold.
pub(crate) fn frame_resp(
    our_index: u32,
    their_index: u32,
    msg2: &[u8],
    peer_mac1: &Mac1Key,
) -> Vec<u8> {
    let header = Packet::pack(&RespHeader::new(our_index, their_index));
    let header: &[u8] = header.as_ref();

    let mut out = Vec::with_capacity(header.len() + msg2.len() + crate::constants::MAC1_LEN);
    out.extend_from_slice(header);
    out.extend_from_slice(msg2);
    let tag = peer_mac1.tag(&out);
    out.extend_from_slice(&tag);
    out
}
