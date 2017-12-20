use std::collections::HashSet;
use rand;

pub struct PktIdGen {
    in_use: HashSet<u16>
}

impl PktIdGen {
    pub fn new() -> PktIdGen {
        PktIdGen { in_use: HashSet::new() }
    }

    pub fn gen(&mut self) -> u16 {
        let mut i = rand::random::<u16>();
        while self.in_use.contains(&i) {
            i = rand::random::<u16>();
        }
        self.in_use.insert(i);
        i
    }

    pub fn rm(&mut self, i: u16) {
        self.in_use.remove(&i);
    }
}
