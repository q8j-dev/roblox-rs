use crate::world::PhysicsWorld;
use parking_lot::Mutex;
use rbx_dom::{DataModel, InstanceId, WorldHooks};
use std::sync::Arc;

pub type SharedWorld = Arc<Mutex<PhysicsWorld>>;

pub fn shared_world() -> SharedWorld {
    Arc::new(Mutex::new(PhysicsWorld::new()))
}

pub struct WorldHooksImpl {
    pub world: SharedWorld,
}

impl WorldHooksImpl {
    pub fn install(dm: &mut DataModel, world: SharedWorld) {
        world.lock().attach(dm);
        dm.set_world_hooks(Box::new(WorldHooksImpl { world }));
    }
}

impl WorldHooks for WorldHooksImpl {
    fn destroy_joints(&mut self, dm: &mut DataModel, id: InstanceId) {
        if let Some(mut w) = self.world.try_lock() {
            w.destroy_joints(dm, id);
        }
    }

    fn join(&mut self, dm: &mut DataModel, id: InstanceId) {
        if let Some(mut w) = self.world.try_lock() {
            w.join(dm, id);
        }
    }

    fn safe_move(&mut self, dm: &mut DataModel, id: InstanceId) {
        if let Some(mut w) = self.world.try_lock() {
            w.safe_move(dm, id);
        }
    }

    fn pv_changed_from_reflection(&mut self, _dm: &mut DataModel, id: InstanceId) {
        if let Some(mut w) = self.world.try_lock() {
            w.wake_part(id);
        }
    }
}
