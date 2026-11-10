use super::{Registry, Ret, ret};
use crate::types::{LEnumItem, enum_item_value};
use mlua::{Lua, Result, Value};
use rbx_dom::InstanceId;

pub fn register(r: &mut Registry) {
    r.add(
        "ThirdPartyUserService",
        &[
            "IsSingleSignOnSupported",
            "IsChatRestrictionSupported",
            "IsAccountSwitchingSupported",
            "HaveActiveUser",
        ],
        unsupported,
    );
    r.add(
        "ThirdPartyUserService",
        &["GetUserPlatformName"],
        platform_name,
    );
    r.add(
        "ThirdPartyUserService",
        &["GetVoiceChatRestrictionStatus"],
        not_restricted,
    );
}

fn unsupported(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, false)
}

fn platform_name(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    ret(lua, "")
}

fn not_restricted(lua: &Lua, _id: InstanceId, _args: &[Value]) -> Result<Ret> {
    match LEnumItem::by_name("ChatRestrictionStatus", "NotRestricted") {
        Some(item) => ret(lua, enum_item_value(lua, item)?),
        None => ret(lua, Value::Nil),
    }
}
