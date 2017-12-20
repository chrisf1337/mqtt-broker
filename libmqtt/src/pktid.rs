use std::u16;
use std::collections::HashSet;
use rand;

pub struct PktIdGen {
    in_use: HashSet<u16>
}

impl PktIdGen {
    pub fn new() -> PktIdGen {
        PktIdGen { in_use: HashSet::new() }
    }

    pub fn gen(&mut self) -> Option<u16> {
        if self.in_use.len() == (u16::MAX as usize) {
            return None;
        }
        let mut i = rand::random::<u16>();
        while self.in_use.contains(&i) {
            i = rand::random::<u16>();
        }
        self.in_use.insert(i);
        Some(i)
    }

    pub fn rm(&mut self, i: u16) {
        self.in_use.remove(&i);
    }
}
