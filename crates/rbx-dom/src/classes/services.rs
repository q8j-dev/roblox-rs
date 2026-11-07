use crate::behavior::Behavior;
use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

pub const SERVICE_CLASSES: &[&str] = &[
    "AdService",
    "AssetService",
    "BadgeService",
    "CacheableContentProvider",
    "ChangeHistoryService",
    "Chat",
    "ChatFilter",
    "ClusterPacketCache",
    "CollectionService",
    "ContentFilter",
    "ContentProvider",
    "ContextActionService",
    "ControllerService",
    "CookiesService",
    "CoreGui",
    "DataStoreService",
    "Debris",
    "DebugSettings",
    "FlagStandService",
    "FlyweightService",
    "FriendService",
    "GamePassService",
    "GameSettings",
    "GamepadService",
    "Geometry",
    "GroupService",
    "GuiService",
    "GuidRegistryService",
    "HapticService",
    "Hopper",
    "HttpRbxApiService",
    "HttpService",
    "InsertService",
    "InstancePacketCache",
    "JointsService",
    "KeyframeSequenceProvider",
    "Lighting",
    "LogService",
    "LoginService",
    "LuaSettings",
    "LuaWebService",
    "MarketplaceService",
    "NetworkClient",
    "NetworkServer",
    "NetworkSettings",
    "NotificationService",
    "OneQuarterClusterPacketCacheBase",
    "PathfindingService",
    "PersonalServerService",
    "PhysicsPacketCache",
    "PhysicsService",
    "PhysicsSettings",
    "PlatformService",
    "Players",
    "PointsService",
    "RenderHooksService",
    "RenderSettings",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "RobloxReplicatedStorage",
    "RunService",
    "RuntimeScriptService",
    "ScriptContext",
    "ScriptInformationProvider",
    "ScriptService",
    "Selection",
    "ServerScriptService",
    "ServerStorage",
    "SocialService",
    "SoundService",
    "SpawnerService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Stats",
    "Studio",
    "TaskScheduler",
    "Teams",
    "TeleportService",
    "TestService",
    "TextService",
    "TimerService",
    "TouchInputService",
    "TweenService",
    "UserGameSettings",
    "UserInputService",
    "VirtualUser",
    "Visit",
    "Workspace",
];

pub fn is_service_class(class: &str) -> bool {
    SERVICE_CLASSES.contains(&class)
}

pub struct ServiceBehavior;

impl Behavior for ServiceBehavior {
    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        child: InstanceId,
        _old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        if id == child && new_parent == Some(dm.root()) {
            dm.lock_parent(id, true);
        }
    }
}

pub struct DataModelBehavior;

impl Behavior for DataModelBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["Workspace", "workspace", "lighting"]
    }

    fn get(&self, dm: &DataModel, _id: InstanceId, prop: &str) -> Option<Variant> {
        match prop {
            "Workspace" | "workspace" => Some(Variant::Ref(dm.find_service("Workspace"))),
            "lighting" => Some(Variant::Ref(dm.find_service("Lighting"))),
            _ => None,
        }
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("DataModel", &DataModelBehavior);
    for &class in SERVICE_CLASSES {
        map.entry(class).or_insert(&ServiceBehavior);
    }
}
