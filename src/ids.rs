use bevy::prelude::*;

pub const MAX_COMPETITORS: usize = 12;

#[derive(Component, Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompetitorId(pub u8);

impl CompetitorId {
    pub const fn new(index: u8) -> Self {
        assert!(index < MAX_COMPETITORS as u8);
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn owner(self) -> OwnerId {
        OwnerId(self.0 + 1)
    }
}

/// Zero is unclaimed; one through twelve map to stable competitor IDs zero through eleven.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct OwnerId(pub u8);

impl OwnerId {
    pub const UNCLAIMED: Self = Self(0);

    pub const fn competitor(self) -> Option<CompetitorId> {
        if self.0 == 0 {
            None
        } else {
            Some(CompetitorId(self.0 - 1))
        }
    }
}
