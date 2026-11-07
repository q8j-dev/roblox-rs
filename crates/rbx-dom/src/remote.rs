use rbx_reflection::{InstanceId, Variant};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RemoteValue {
    Value(Variant),
    Array(Vec<RemoteValue>),
    Map(Vec<(String, RemoteValue)>),
    Enum { ty: String, value: u32 },
}

impl RemoteValue {
    pub fn map_refs(
        self,
        f: &mut impl FnMut(Option<InstanceId>) -> Option<InstanceId>,
    ) -> RemoteValue {
        match self {
            RemoteValue::Value(Variant::Ref(r)) => RemoteValue::Value(Variant::Ref(f(r))),
            RemoteValue::Value(v) => RemoteValue::Value(v),
            RemoteValue::Enum { ty, value } => RemoteValue::Enum { ty, value },
            RemoteValue::Array(items) => {
                RemoteValue::Array(items.into_iter().map(|v| v.map_refs(f)).collect())
            }
            RemoteValue::Map(items) => {
                RemoteValue::Map(items.into_iter().map(|(k, v)| (k, v.map_refs(f))).collect())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteKind {
    Event,
    Invoke(u32),
    Return(u32),
    Fail(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteTarget {
    Server,
    Player(InstanceId),
    AllPlayers,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteCall {
    pub instance: InstanceId,
    pub target: RemoteTarget,
    pub kind: RemoteKind,
    pub args: Vec<RemoteValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteDelivery {
    pub instance: InstanceId,
    pub player: Option<InstanceId>,
    pub kind: RemoteKind,
    pub args: Vec<RemoteValue>,
}
