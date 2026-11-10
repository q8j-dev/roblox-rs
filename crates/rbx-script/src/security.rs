use rbx_reflection::Security;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Identity {
    Anonymous,
    LocalGui,
    GameScript,
    GameScriptInRobloxPlace,
    RobloxGameScript,
    CmdLine,
    StudioPlugin,
    Com,
    WebService,
    Replicator,
}

impl Identity {
    pub fn from_level(level: i32) -> Identity {
        match level {
            0 => Identity::Anonymous,
            1 => Identity::LocalGui,
            3 => Identity::GameScriptInRobloxPlace,
            4 => Identity::RobloxGameScript,
            5 => Identity::CmdLine,
            6 => Identity::StudioPlugin,
            7 => Identity::Com,
            8 => Identity::WebService,
            9 => Identity::Replicator,
            _ => Identity::GameScript,
        }
    }

    pub fn level(self) -> i32 {
        match self {
            Identity::Anonymous => 0,
            Identity::LocalGui => 1,
            Identity::GameScript => 2,
            Identity::GameScriptInRobloxPlace => 3,
            Identity::RobloxGameScript => 4,
            Identity::CmdLine => 5,
            Identity::StudioPlugin => 6,
            Identity::Com => 7,
            Identity::WebService => 8,
            Identity::Replicator => 9,
        }
    }

    pub fn allows(self, p: Security) -> bool {
        let p = if p == Security::TestLocalUser {
            Security::LocalUser
        } else {
            p
        };
        if p == Security::None {
            return true;
        }
        match self {
            Identity::Anonymous | Identity::GameScript => false,
            Identity::StudioPlugin => p == Security::Plugin,
            Identity::GameScriptInRobloxPlace => p == Security::RobloxPlace,
            Identity::RobloxGameScript => matches!(
                p,
                Security::Plugin
                    | Security::RobloxPlace
                    | Security::LocalUser
                    | Security::RobloxScript
            ),
            Identity::LocalGui | Identity::CmdLine => {
                matches!(
                    p,
                    Security::Plugin | Security::RobloxPlace | Security::LocalUser
                )
            }
            Identity::Replicator => matches!(
                p,
                Security::RobloxPlace | Security::WritePlayer | Security::RobloxScript
            ),
            Identity::Com | Identity::WebService => true,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Identity::Anonymous => "Anonymous",
            Identity::LocalGui => "LocalGUI",
            Identity::GameScript => "GameScript",
            Identity::GameScriptInRobloxPlace => "GameScriptInRobloxPlace",
            Identity::RobloxGameScript => "RobloxGameScript",
            Identity::CmdLine => "CmdLine",
            Identity::StudioPlugin => "StudioPlugin",
            Identity::Com => "COM",
            Identity::WebService => "WebService",
            Identity::Replicator => "Replicator",
        }
    }
}

pub fn permission_name(p: Security) -> &'static str {
    match p {
        Security::None => "None",
        Security::Plugin => "Plugin",
        Security::LocalUser => "LocalUser",
        Security::WritePlayer => "WritePlayer",
        Security::RobloxScript => "RobloxScript",
        Security::Roblox => "Roblox",
        Security::RobloxPlace => "RobloxPlace",
        Security::TestLocalUser => "LocalUser",
        Security::NotAccessible => "NotAccessible",
    }
}
