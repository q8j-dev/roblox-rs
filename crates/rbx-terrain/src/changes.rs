use rbx_math::IVec3;

const MAX_ENTRIES: usize = 1 << 16;

#[derive(Clone, Debug, Default)]
pub struct ChangeLog {
    base: u64,
    entries: Vec<(IVec3, IVec3)>,
}

pub enum Changes<'a> {
    Since(&'a [(IVec3, IVec3)]),
    Everything,
}

impl ChangeLog {
    pub fn push(&mut self, min: IVec3, max: IVec3) {
        if self.entries.len() >= MAX_ENTRIES {
            self.base += self.entries.len() as u64 + 1;
            self.entries.clear();
        }
        self.entries.push((min, max));
    }

    pub fn generation(&self) -> u64 {
        self.base + self.entries.len() as u64
    }

    pub fn since(&self, generation: u64) -> Changes<'_> {
        if generation < self.base {
            return Changes::Everything;
        }
        let start = (generation - self.base) as usize;
        if start > self.entries.len() {
            return Changes::Everything;
        }
        Changes::Since(&self.entries[start..])
    }

    pub fn reset(&mut self) {
        self.base += self.entries.len() as u64 + 1;
        self.entries.clear();
    }
}
