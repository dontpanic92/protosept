//! Stable-handle, generationally-versioned slab storage for boxed values.
//!
//! The runtime's box storage previously compacted on every GC sweep, which
//! made `Data::BoxRef` indices unstable across collections. Any Rust code
//! that cached a `Data` value across a script call risked dispatching into
//! a different heap object after a mid-call GC.
//!
//! [`BoxHeap`] solves this by giving every slot a stable index for its
//! lifetime. Freed slots are returned to a free list (so memory is reused),
//! and each slot carries a `u32` generation that is bumped on free. Every
//! [`Data::BoxRef`] / [`Data::ProtoBoxRef`] / [`Data::ProtoRefRef`] carries
//! the slot's generation at allocation time; [`BoxHeap::get`] /
//! [`BoxHeap::get_mut`] validate the generation and return
//! [`RuntimeError::StaleBoxHandle`] if it does not match. This makes
//! use-after-free fail fast instead of silently aliasing whatever value now
//! occupies the recycled slot.
//!
//! There is no compaction. Memory sits at the live high-water mark; for the
//! workloads this runtime targets that is fine. If true compaction is ever
//! needed, the layout can be wrapped in a stable handle table without
//! changing the [`Data`] enum or the call sites that go through this API.

use crate::errors::RuntimeError;

use super::data::{ContextResult, Data};

/// Slab-allocated, generationally-versioned storage for boxed values.
#[derive(Debug)]
pub struct BoxHeap {
    slots: Vec<Option<Data>>,
    gens: Vec<u32>,
    /// Indices of currently free slots, available for reuse before growing
    /// `slots`. We use a plain `Vec` (LIFO) rather than an intrusive
    /// linked list so that the free representation does not need to live
    /// inside the slot itself; this keeps the per-slot `Option<Data>`
    /// shape simple for the GC mark/sweep traversal.
    free_list: Vec<u32>,
    live_count: usize,
}

impl Default for BoxHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxHeap {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            gens: Vec::new(),
            free_list: Vec::new(),
            live_count: 0,
        }
    }

    /// Allocate a new box slot with `value` and return the stable
    /// `(idx, generation)` handle. The caller is responsible for stamping
    /// `generation` onto any `Data::*BoxRef` it constructs.
    pub fn alloc(&mut self, value: Data) -> (u32, u32) {
        if let Some(idx) = self.free_list.pop() {
            self.slots[idx as usize] = Some(value);
            self.live_count += 1;
            (idx, self.gens[idx as usize])
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Some(value));
            self.gens.push(0);
            self.live_count += 1;
            (idx, 0)
        }
    }

    /// Read the live slot at `idx`, validating the generation. Returns
    /// `RuntimeError::StaleBoxHandle` if the generation does not match —
    /// typically because the slot was freed (and possibly reused) since
    /// the handle was minted.
    pub fn get(&self, idx: u32, generation: u32) -> ContextResult<&Data> {
        let slot = self.slots.get(idx as usize).ok_or_else(|| {
            RuntimeError::Other(format!("box index {} out of bounds", idx))
        })?;
        let actual_gen = self.gens[idx as usize];
        if actual_gen != generation {
            return Err(RuntimeError::StaleBoxHandle {
                idx,
                expected_gen: generation,
                actual_gen,
            });
        }
        slot.as_ref().ok_or(RuntimeError::StaleBoxHandle {
            idx,
            expected_gen: generation,
            actual_gen,
        })
    }

    /// Mutable counterpart to [`BoxHeap::get`].
    pub fn get_mut(&mut self, idx: u32, generation: u32) -> ContextResult<&mut Data> {
        let actual_gen = *self.gens.get(idx as usize).ok_or_else(|| {
            RuntimeError::Other(format!("box index {} out of bounds", idx))
        })?;
        if actual_gen != generation {
            return Err(RuntimeError::StaleBoxHandle {
                idx,
                expected_gen: generation,
                actual_gen,
            });
        }
        let slot = &mut self.slots[idx as usize];
        slot.as_mut().ok_or(RuntimeError::StaleBoxHandle {
            idx,
            expected_gen: generation,
            actual_gen,
        })
    }

    /// Read the live slot at `idx` without validating a generation. Used
    /// by GC traversal where the mark phase has already proved the slot
    /// is reachable and we don't have a generation to check against.
    /// Returns `None` if the slot is free.
    pub fn get_unchecked(&self, idx: u32) -> Option<&Data> {
        self.slots.get(idx as usize).and_then(Option::as_ref)
    }

    /// Mutable counterpart to [`BoxHeap::get_unchecked`].
    pub fn get_unchecked_mut(&mut self, idx: u32) -> Option<&mut Data> {
        self.slots.get_mut(idx as usize).and_then(Option::as_mut)
    }

    /// Current generation of the slot at `idx`, regardless of whether it
    /// is live or free. Returns `None` if `idx` is out of bounds.
    pub fn current_gen(&self, idx: u32) -> Option<u32> {
        self.gens.get(idx as usize).copied()
    }

    /// Total number of slots ever allocated (live + free). Use
    /// [`BoxHeap::live_count`] for the count of currently live slots.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn live_count(&self) -> usize {
        self.live_count
    }

    /// Free the slot at `idx`. Bumps the slot's generation so any
    /// outstanding handle is detected as stale on next dereference.
    /// Returns the previously-stored value, if any. Calling `free` on
    /// an already-free slot is a no-op.
    pub fn free(&mut self, idx: u32) -> Option<Data> {
        let slot = self.slots.get_mut(idx as usize)?;
        let prev = slot.take();
        if prev.is_some() {
            self.gens[idx as usize] = self.gens[idx as usize].wrapping_add(1);
            self.free_list.push(idx);
            self.live_count -= 1;
        }
        prev
    }

    /// Iterate over `(idx, &Data)` pairs for every currently-live slot.
    pub fn iter_live(&self) -> impl Iterator<Item = (u32, &Data)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_ref().map(|v| (idx as u32, v)))
    }

    /// Iterate mutably over `(idx, &mut Data)` pairs for every
    /// currently-live slot.
    pub fn iter_live_mut(&mut self) -> impl Iterator<Item = (u32, &mut Data)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_mut().map(|v| (idx as u32, v)))
    }
}
