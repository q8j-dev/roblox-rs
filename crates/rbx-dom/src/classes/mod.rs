pub(crate) mod custom_events;
mod humanoid;
mod instance;
pub(crate) mod parts;
pub(crate) mod seat;
mod services;
pub(crate) mod terrain;
pub(crate) mod tool;

use crate::behavior::Behavior;
use std::collections::HashMap;

pub use services::{SERVICE_CLASSES, is_service_class};

pub fn registry() -> HashMap<&'static str, &'static dyn Behavior> {
    let mut map: HashMap<&'static str, &'static dyn Behavior> = HashMap::new();
    map.insert("Instance", &instance::InstanceBehavior);
    parts::register(&mut map);
    humanoid::register(&mut map);
    services::register(&mut map);
    terrain::register(&mut map);
    custom_events::register(&mut map);
    tool::register(&mut map);
    seat::register(&mut map);
    map
}
