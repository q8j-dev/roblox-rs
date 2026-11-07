use crate::wire::{Channel, Message, WireError, decode, encode};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub trait Connection: Send {
    fn send_bytes(&mut self, channel: Channel, bytes: Vec<u8>);
    fn recv_bytes(&mut self) -> Option<Vec<u8>>;
    fn is_closed(&self) -> bool;
    fn close(&mut self);

    fn describe(&self) -> String {
        String::new()
    }

    fn send(&mut self, msg: &Message) -> usize {
        let bytes = encode(msg);
        let n = bytes.len();
        self.send_bytes(msg.channel(), bytes);
        n
    }

    fn recv(&mut self) -> Option<Result<Message, WireError>> {
        self.recv_bytes().map(|b| decode(&b))
    }
}

type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;

pub struct MemoryConnection {
    inbox: Queue,
    outbox: Queue,
    closed: Arc<AtomicBool>,
    pub sent_bytes: u64,
}

pub fn memory_pair() -> (MemoryConnection, MemoryConnection) {
    let a: Queue = Arc::default();
    let b: Queue = Arc::default();
    let closed = Arc::new(AtomicBool::new(false));
    (
        MemoryConnection {
            inbox: a.clone(),
            outbox: b.clone(),
            closed: closed.clone(),
            sent_bytes: 0,
        },
        MemoryConnection {
            inbox: b,
            outbox: a,
            closed,
            sent_bytes: 0,
        },
    )
}

impl Connection for MemoryConnection {
    fn send_bytes(&mut self, _channel: Channel, bytes: Vec<u8>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.sent_bytes += bytes.len() as u64;
        self.outbox.lock().push_back(bytes);
    }

    fn recv_bytes(&mut self) -> Option<Vec<u8>> {
        self.inbox.lock().pop_front()
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire) && self.inbox.lock().is_empty()
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}
