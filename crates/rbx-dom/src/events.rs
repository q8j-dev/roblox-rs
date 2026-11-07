use rbx_reflection::InstanceId;

#[derive(Clone, Debug, PartialEq)]
pub enum DomEvent {
    Created {
        id: InstanceId,
    },
    PropertyChanged {
        id: InstanceId,
        property: &'static str,
    },
    ChildAdded {
        parent: InstanceId,
        child: InstanceId,
    },
    ChildRemoved {
        parent: InstanceId,
        child: InstanceId,
    },
    DescendantAdded {
        ancestor: InstanceId,
        descendant: InstanceId,
    },
    DescendantRemoving {
        ancestor: InstanceId,
        descendant: InstanceId,
    },
    AncestryChanged {
        id: InstanceId,
        child: InstanceId,
        parent: Option<InstanceId>,
    },
    Destroyed {
        id: InstanceId,
    },
}

#[derive(Debug, Default)]
pub struct EventLog {
    events: Vec<DomEvent>,
    base: u64,
    cursors: Vec<Option<u64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCursor(usize);

impl EventLog {
    pub fn push(&mut self, e: DomEvent) {
        if self.cursors.iter().all(Option::is_none) {
            return;
        }
        self.events.push(e);
    }

    pub fn subscribe(&mut self) -> EventCursor {
        let pos = self.base + self.events.len() as u64;
        if let Some(i) = self.cursors.iter().position(Option::is_none) {
            self.cursors[i] = Some(pos);
            EventCursor(i)
        } else {
            self.cursors.push(Some(pos));
            EventCursor(self.cursors.len() - 1)
        }
    }

    pub fn unsubscribe(&mut self, cursor: EventCursor) {
        if let Some(slot) = self.cursors.get_mut(cursor.0) {
            *slot = None;
        }
        self.compact();
    }

    pub fn drain(&mut self, cursor: EventCursor) -> Vec<DomEvent> {
        let Some(Some(pos)) = self.cursors.get(cursor.0).copied() else {
            return Vec::new();
        };
        let start = (pos - self.base) as usize;
        let out = self.events[start.min(self.events.len())..].to_vec();
        self.cursors[cursor.0] = Some(self.base + self.events.len() as u64);
        self.compact();
        out
    }

    pub fn pending(&self, cursor: EventCursor) -> usize {
        match self.cursors.get(cursor.0).copied().flatten() {
            Some(pos) => (self.base + self.events.len() as u64 - pos) as usize,
            None => 0,
        }
    }

    fn compact(&mut self) {
        let min = self.cursors.iter().flatten().min().copied();
        match min {
            None => {
                self.base += self.events.len() as u64;
                self.events.clear();
            }
            Some(m) => {
                let drop = (m - self.base) as usize;
                if drop > 0 && drop * 2 >= self.events.len() {
                    self.events.drain(..drop);
                    self.base = m;
                }
            }
        }
    }
}
