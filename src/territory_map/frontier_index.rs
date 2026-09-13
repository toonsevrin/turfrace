use crate::ids::MAX_COMPETITORS;
use std::collections::BTreeSet;

/// Sparse, row-major frontier sample indices for each owner.
///
/// Ordered sets preserve the former scan order without shifting a whole
/// owner's frontier on every cell mutation during a large capture.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FrontierIndex {
    owners: [BTreeSet<usize>; MAX_COMPETITORS],
}

impl FrontierIndex {
    pub(super) fn clear(&mut self) {
        for indices in &mut self.owners {
            indices.clear();
        }
    }

    pub(super) fn owner_indices(&self, owner: usize) -> impl Iterator<Item = &usize> {
        self.owners[owner].iter()
    }

    pub(super) fn update(&mut self, index: usize, old_bits: u16, new_bits: u16) {
        if old_bits == new_bits {
            return;
        }
        for owner in 0..MAX_COMPETITORS {
            let bit = 1u16 << owner;
            match (old_bits & bit != 0, new_bits & bit != 0) {
                (true, false) => {
                    assert!(
                        self.owners[owner].remove(&index),
                        "frontier index missing removed cell"
                    );
                }
                (false, true) => {
                    self.owners[owner].insert(index);
                }
                _ => {}
            }
        }
    }
}
