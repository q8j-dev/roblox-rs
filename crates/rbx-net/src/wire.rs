use rbx_dom::Variant;
use rbx_math::{CFrame, Vec3};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const SERVER_PEER: PeerId = 0;

pub type PeerId = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NetId(pub u64);

impl NetId {
    pub fn new(peer: PeerId, local: u64) -> NetId {
        NetId(((peer as u64) << 48) | (local & 0xFFFF_FFFF_FFFF))
    }

    pub fn peer(self) -> PeerId {
        (self.0 >> 48) as PeerId
    }

    pub fn local(self) -> u64 {
        self.0 & 0xFFFF_FFFF_FFFF
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    Reliable,
    Unreliable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewInstance {
    pub id: NetId,
    pub class: String,
    pub name: String,
    pub parent: NetId,
    pub properties: Vec<(String, Variant)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicsState {
    pub id: NetId,
    pub cframe: CFrame,
    pub velocity: Vec3,
    pub rot_velocity: Vec3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Message {
    Hello {
        protocol: u32,
    },
    Welcome {
        peer: PeerId,
        filtering: bool,
    },
    Reject {
        reason: String,
    },
    Service {
        id: NetId,
        class: String,
        properties: Vec<(String, Variant)>,
    },
    New(Vec<NewInstance>),
    Property {
        id: NetId,
        name: String,
        value: Variant,
    },
    Parent {
        id: NetId,
        parent: NetId,
    },
    Delete {
        id: NetId,
    },
    Event {
        id: NetId,
        name: String,
        args: Vec<Variant>,
    },
    Invoke {
        call: u32,
        id: NetId,
        args: Vec<Variant>,
    },
    InvokeResult {
        call: u32,
        result: Result<Vec<Variant>, String>,
    },
    Ownership {
        id: NetId,
        owner: PeerId,
    },
    Physics(Vec<PhysicsState>),
    JoinComplete,
    Ping {
        nonce: u32,
    },
    Pong {
        nonce: u32,
    },
}

impl Message {
    pub fn channel(&self) -> Channel {
        match self {
            Message::Physics(_) => Channel::Unreliable,
            _ => Channel::Reliable,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("malformed packet: {0}")]
    Malformed(#[from] postcard::Error),
}

pub fn encode(msg: &Message) -> Vec<u8> {
    match postcard::to_stdvec(msg) {
        Ok(v) => v,
        Err(e) => panic!("message encoding cannot fail for owned data: {e}"),
    }
}

pub fn decode(bytes: &[u8]) -> Result<Message, WireError> {
    Ok(postcard::from_bytes(bytes)?)
}
