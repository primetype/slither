//! §9.1 — stream identifiers and the four ID spaces.
//!
//! A stream ID is a varint whose two low bits tag the stream and whose
//! remaining 60 bits are a per-space index allocated from 0:
//!
//! | Bit | Meaning |
//! |---|---|
//! | `0x01` | opener: 0 = the connection initiator, 1 = the acceptor |
//! | `0x02` | direction: 0 = bidirectional, 1 = unidirectional |
//!
//! # The two ceilings are different numbers
//!
//! §8.1's varint holds 62 bits and 2 + 60 = 62, so `index = id >> 2` can
//! never exceed 2⁶⁰ − 1 for any *decodable* `stream_id`: [`from_u64`] is
//! total and there is no reachable "index too large" case on the STREAM
//! path. The ceiling is reachable only on MAX_STREAMS, where §8.4 states it
//! as a structural error (`max` > 2⁶⁰) — and the boundary is `>`, not `≥`,
//! because §10.4's *"opening stream index `i` requires cumulative limit >
//! `i`"* makes `max = 2⁶⁰` exactly the limit that admits the largest
//! representable index.
//!
//! [`from_u64`]: StreamId::from_u64
//!
//! # Parity is read, never re-derived
//!
//! **[ruling 106]** The opener bit refers to the roles of the connection's
//! *establishment*, and §6.6 step 4 admits a peer that **dialled** as the
//! responder. [`Opener::of_role`] is the only conversion, and its input is
//! the [`Role`] the `Install` carried — never "I was created by
//! `connect()`".

use crate::core::Role;

/// The opener bit: 0 = the connection initiator, 1 = the acceptor. §9.1.
const OPENER_BIT: u64 = 0x01;

/// The direction bit: 0 = bidirectional, 1 = unidirectional. §9.1.
const DIR_BIT: u64 = 0x02;

/// The largest §9.1 index: 60 bits.
pub(crate) const INDEX_MAX: u64 = (1u64 << 60) - 1;

/// The cumulative stream limit that admits every representable index.
///
/// §8.4: *"Structural error: `max` > 2⁶⁰"* — so `2⁶⁰` itself is legal.
pub(crate) const MAX_STREAMS_CEILING: u64 = 1u64 << 60;

/// §9.1's wire stream identifier.
///
/// **[RATIFIED 2026/08/15 — ruling 101]** Public: §16.2's
/// `SendStream::id(&self) -> Option<StreamId>` makes it reachable whether
/// or not §9.1 mints the type. There is deliberately **no public
/// constructor** — an application must not be able to mint an id for a
/// stream it does not own.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StreamId(u64);

impl StreamId {
    /// The per-space index — `id >> 2`. §9.1.
    pub fn index(self) -> u64 {
        self.0 >> 2
    }

    /// Which of §9.1's two directions this stream is.
    pub fn dir(self) -> Dir {
        if self.0 & DIR_BIT == 0 {
            Dir::Bi
        } else {
            Dir::Uni
        }
    }

    /// Whether the **connection initiator** opened this stream. §9.1.
    ///
    /// Not "whether *we* opened it": the answer is a property of the id and
    /// the same on both ends.
    pub fn initiated_by_connection_initiator(self) -> bool {
        self.0 & OPENER_BIT == 0
    }

    /// The varint value that goes on the wire.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// A stream id read off the wire.
    ///
    /// Total: every decodable `stream_id` is at most 2⁶² − 1, so its index
    /// is at most 2⁶⁰ − 1 by construction (see the module docs).
    pub(crate) fn from_u64(v: u64) -> Self {
        Self(v)
    }

    /// Mint the id for a stream this connection is allocating.
    ///
    /// `index` is bounded by §10.4's cumulative limit, which is itself
    /// bounded by [`MAX_STREAMS_CEILING`], so the shift cannot lose bits on
    /// any reachable path.
    pub(crate) fn new(index: u64, dir: Dir, opener: Opener) -> Self {
        debug_assert!(
            index <= INDEX_MAX,
            "§9.1: the index is 60 bits and §10.4's limit is what bounds it"
        );
        let dir_bit = match dir {
            Dir::Bi => 0,
            Dir::Uni => DIR_BIT,
        };
        let opener_bit = match opener {
            Opener::Initiator => 0,
            Opener::Responder => OPENER_BIT,
        };
        Self((index << 2) | dir_bit | opener_bit)
    }

    /// Which end opened it.
    pub(crate) fn opener(self) -> Opener {
        if self.initiated_by_connection_initiator() {
            Opener::Initiator
        } else {
            Opener::Responder
        }
    }

    /// The §9.1 space this id belongs to.
    pub(crate) fn space(self) -> Space {
        Space {
            dir: self.dir(),
            opener: self.opener(),
        }
    }
}

impl ::core::fmt::Display for StreamId {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// §9.1's direction.
///
/// **[RATIFIED 2026/08/15 — ruling 101]** Public, and deliberately **not**
/// `#[non_exhaustive]`: two variants, closed by the protocol, and a wildcard
/// arm forced on every user forever for a set that cannot grow is the wrong
/// trade.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dir {
    /// Bidirectional: both ends hold a send half and a receive half.
    Bi,
    /// Unidirectional: the opener sends, the peer receives.
    Uni,
}

impl Dir {
    /// This direction's slot in a two-element table.
    pub(crate) fn slot(self) -> usize {
        match self {
            Dir::Bi => 0,
            Dir::Uni => 1,
        }
    }

    /// Both directions, in slot order.
    pub(crate) const ALL: [Dir; 2] = [Dir::Bi, Dir::Uni];
}

/// Which end of the *connection* opened a stream. §9.1.
///
/// Crate-internal: it is [`Role`] seen from §9.1's angle, and the public
/// surface asks the question as
/// [`StreamId::initiated_by_connection_initiator`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Opener {
    /// The connection initiator — the dialler, or §6.7's tie-break winner.
    Initiator,
    /// The acceptor.
    Responder,
}

impl Opener {
    /// **[ruling 106]** The only conversion, and its input is the role the
    /// `Install` carried.
    pub(crate) fn of_role(role: Role) -> Self {
        match role {
            Role::Initiator => Opener::Initiator,
            Role::Responder => Opener::Responder,
        }
    }

    /// The other end.
    pub(crate) fn peer(self) -> Self {
        match self {
            Opener::Initiator => Opener::Responder,
            Opener::Responder => Opener::Initiator,
        }
    }
}

/// One of §9.1's four independent ID spaces.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Space {
    /// The direction half of the tag.
    pub dir: Dir,
    /// The opener half of the tag.
    pub opener: Opener,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §9.1's table, as bit masks on the *encoded* value.
    #[test]
    fn the_two_low_bits_are_the_tag_and_the_rest_is_the_index() {
        let cases = [
            (Dir::Bi, Opener::Initiator, 0b00u64),
            (Dir::Bi, Opener::Responder, 0b01),
            (Dir::Uni, Opener::Initiator, 0b10),
            (Dir::Uni, Opener::Responder, 0b11),
        ];
        for (dir, opener, tag) in cases {
            for index in [0u64, 1, 5, 31, 128, INDEX_MAX] {
                let id = StreamId::new(index, dir, opener);
                assert_eq!(id.as_u64(), (index << 2) | tag);
                assert_eq!(id.index(), index);
                assert_eq!(id.dir(), dir);
                assert_eq!(id.opener(), opener);
                assert_eq!(
                    id.initiated_by_connection_initiator(),
                    opener == Opener::Initiator
                );
                assert_eq!(StreamId::from_u64(id.as_u64()), id);
            }
        }
    }

    /// The largest decodable stream id yields the largest legal index —
    /// which is what makes [`StreamId::from_u64`] total.
    #[test]
    fn the_largest_decodable_id_yields_the_largest_index() {
        let largest = crate::varint::VarInt::MAX_VALUE;
        assert_eq!(StreamId::from_u64(largest).index(), INDEX_MAX);
        assert_eq!(MAX_STREAMS_CEILING, INDEX_MAX + 1);
    }

    #[test]
    fn display_is_the_decimal_wire_value() {
        assert_eq!(
            StreamId::new(3, Dir::Uni, Opener::Responder).to_string(),
            "15"
        );
    }

    /// **[ruling 106]** The role is the only input to parity.
    #[test]
    fn opener_comes_from_the_installed_role() {
        assert_eq!(Opener::of_role(Role::Initiator), Opener::Initiator);
        assert_eq!(Opener::of_role(Role::Responder), Opener::Responder);
        assert_eq!(Opener::Initiator.peer(), Opener::Responder);
        assert_eq!(Opener::Responder.peer(), Opener::Initiator);
    }
}
