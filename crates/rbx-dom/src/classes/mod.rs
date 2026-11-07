mod instance;
mod parts;
mod services;

use crate::behavior::Behavior;
use std::collections::HashMap;

pub use services::{SERVICE_CLASSES, is_service_class};

pub fn registry() -> HashMap<&'static str, &'static dyn Behavior> {
    let mut map: HashMap<&'static str, &'static dyn Behavior> = HashMap::new();
    map.insert("Instance", &instance::InstanceBehavior);
    parts::register(&mut map);
    services::register(&mut map);
    map
}
