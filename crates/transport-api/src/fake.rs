//! Bounded in-memory `SessionTransport` pair for deterministic scripted-peer
//! tests.
//!
//! This lets a test script a fake phone at the frame/transport level — no
//! USB, no TCP, no real device — while exercising the exact same
//! `SessionTransport` contract that `transport-usb` and `transport-tcp`
//! implement against real hardware. It is not a protocol fake: it only
//! moves bytes, with the same bounded-queue discipline the architecture
//! requires elsewhere (a saturated direction is a hard error, never
//! unbounded growth).
//!
//! Feature-gated behind `test-support`; never linked into a release binary.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::{SessionTransport, TransportError};

struct Shared {
    /// Bytes waiting to be returned by the transport side's `receive()`.
    inbound: VecDeque<u8>,
    /// Bytes the transport side has sent, waiting to be observed by the peer.
    outbound: VecDeque<u8>,
    max_buffered: usize,
}

/// The transport-facing end of the pair. Implements [`SessionTransport`].
pub struct Transport {
    shared: Rc<RefCell<Shared>>,
}

/// The script-facing end of the pair: feeds bytes for `receive()` to return
/// and observes what the transport side sends.
pub struct Peer {
    shared: Rc<RefCell<Shared>>,
}

/// Build a bounded in-memory transport pair.
///
/// `max_buffered` caps each direction independently. Exceeding it is a
/// `TransportError`, not silent growth, matching this project's bounded-queue
/// rule (see `ARCHITECTURE.md` section 6).
#[must_use]
pub fn pair(max_buffered: usize) -> (Transport, Peer) {
    let shared = Rc::new(RefCell::new(Shared {
        inbound: VecDeque::new(),
        outbound: VecDeque::new(),
        max_buffered,
    }));
    (
        Transport {
            shared: Rc::clone(&shared),
        },
        Peer { shared },
    )
}

impl SessionTransport for Transport {
    /// Returns `TransportError::TimedOut` when nothing is queued, matching
    /// the blocking-with-timeout convention real transports use (see the
    /// `Err(TransportError::TimedOut) => continue` pattern in
    /// `live_probe.rs`), rather than blocking or returning `Ok(0)`.
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        let mut shared = self.shared.borrow_mut();
        if shared.inbound.is_empty() {
            return Err(TransportError::TimedOut);
        }
        let mut read = 0;
        while read < buffer.len() {
            let Some(byte) = shared.inbound.pop_front() else {
                break;
            };
            buffer[read] = byte;
            read += 1;
        }
        Ok(read)
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let mut shared = self.shared.borrow_mut();
        if shared.outbound.len() + bytes.len() > shared.max_buffered {
            return Err(TransportError::Io(
                "fake transport outbound buffer exceeded its bound".into(),
            ));
        }
        shared.outbound.extend(bytes.iter().copied());
        Ok(())
    }
}

impl Peer {
    /// Queue bytes for the transport side's next `receive()` call(s).
    pub fn push_inbound(&self, bytes: &[u8]) -> Result<(), TransportError> {
        let mut shared = self.shared.borrow_mut();
        if shared.inbound.len() + bytes.len() > shared.max_buffered {
            return Err(TransportError::Io(
                "fake transport inbound buffer exceeded its bound".into(),
            ));
        }
        shared.inbound.extend(bytes.iter().copied());
        Ok(())
    }

    /// Drain and return everything the transport side has sent since the
    /// last drain.
    #[must_use]
    pub fn drain_outbound(&self) -> Vec<u8> {
        let mut shared = self.shared.borrow_mut();
        shared.outbound.drain(..).collect()
    }

    /// True if the transport side has sent nothing since the last drain.
    #[must_use]
    pub fn outbound_is_empty(&self) -> bool {
        self.shared.borrow().outbound.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_times_out_when_nothing_queued() {
        let (mut transport, _peer) = pair(1024);
        let mut buffer = [0_u8; 8];
        assert_eq!(
            transport.receive(&mut buffer),
            Err(TransportError::TimedOut)
        );
    }

    #[test]
    fn round_trips_bytes_in_both_directions() {
        let (mut transport, peer) = pair(1024);

        peer.push_inbound(&[1, 2, 3, 4]).expect("push inbound");
        let mut buffer = [0_u8; 8];
        let read = transport.receive(&mut buffer).expect("receive");
        assert_eq!(&buffer[..read], &[1, 2, 3, 4]);

        transport.send_all(&[9, 8, 7]).expect("send");
        assert_eq!(peer.drain_outbound(), vec![9, 8, 7]);
        assert!(peer.outbound_is_empty());
    }

    #[test]
    fn receive_only_fills_the_provided_buffer_and_keeps_the_remainder_queued() {
        let (mut transport, peer) = pair(1024);
        peer.push_inbound(&[1, 2, 3, 4, 5]).expect("push inbound");

        let mut small = [0_u8; 2];
        assert_eq!(transport.receive(&mut small).expect("first receive"), 2);
        assert_eq!(small, [1, 2]);

        let mut rest = [0_u8; 8];
        assert_eq!(transport.receive(&mut rest).expect("second receive"), 3);
        assert_eq!(&rest[..3], &[3, 4, 5]);
    }

    #[test]
    fn outbound_is_bounded_and_never_grows_without_limit() {
        let (mut transport, _peer) = pair(4);
        transport.send_all(&[1, 2, 3, 4]).expect("fits exactly");
        assert_eq!(
            transport.send_all(&[5]),
            Err(TransportError::Io(
                "fake transport outbound buffer exceeded its bound".into()
            ))
        );
    }

    #[test]
    fn inbound_is_bounded_and_never_grows_without_limit() {
        let (_transport, peer) = pair(4);
        peer.push_inbound(&[1, 2, 3, 4]).expect("fits exactly");
        assert_eq!(
            peer.push_inbound(&[5]),
            Err(TransportError::Io(
                "fake transport inbound buffer exceeded its bound".into()
            ))
        );
    }
}
