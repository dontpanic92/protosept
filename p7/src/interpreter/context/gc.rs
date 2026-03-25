use std::collections::HashSet;

use super::Context;
use super::data::Data;

impl Context {
    /// Mark-and-sweep garbage collector for box heap
    /// This method performs a full GC cycle
    pub fn collect_garbage(&mut self) {
        // Mark phase: identify all reachable boxes
        let mut marked = HashSet::new();
        self.mark_reachable(&mut marked);

        // Sweep phase: remove unreachable boxes and compact the heap
        self.sweep(&marked);
    }

    /// Mark phase: traverse all roots and mark reachable boxes
    fn mark_reachable(&self, marked: &mut HashSet<u32>) {
        // Mark from all stack frames
        for frame in &self.stack {
            // Mark boxes in evaluation stack
            for data in &frame.stack {
                self.mark_data(data, marked);
            }
            // Mark boxes in local variables
            for data in &frame.locals {
                self.mark_data(data, marked);
            }
            // Mark boxes in parameters
            for data in &frame.params {
                self.mark_data(data, marked);
            }
        }

        // Mark from heap-allocated structs (they may contain box references)
        for struct_obj in &self.heap {
            for data in &struct_obj.fields {
                self.mark_data(data, marked);
            }
        }

        // Mark from module-level variables (thread-local globals)
        for vars in &self.module_vars {
            for data in vars {
                self.mark_data(data, marked);
            }
        }
    }

    /// Recursively mark a data value and any boxes it references
    fn mark_data(&self, data: &Data, marked: &mut HashSet<u32>) {
        match data {
            Data::BoxRef(idx) | Data::ProtoBoxRef { box_idx: idx, .. } => {
                // If we haven't marked this box yet, mark it and recursively mark its contents
                if marked.insert(*idx) {
                    // Get the box contents and recursively mark
                    if let Some(box_data) = self.box_heap.get(*idx as usize) {
                        self.mark_data(box_data, marked);
                    }
                }
            }
            Data::StructRef(idx) => {
                if let Some(struct_obj) = self.heap.get(*idx as usize) {
                    for field_data in &struct_obj.fields {
                        self.mark_data(field_data, marked);
                    }
                }
            }
            Data::Array(elements) => {
                for elem in elements {
                    self.mark_data(elem, marked);
                }
            }
            Data::Some(inner) => {
                self.mark_data(inner, marked);
            }
            Data::Closure { captures, .. } => {
                for capture in captures {
                    self.mark_data(capture, marked);
                }
            }
            Data::Tuple(elements) => {
                for elem in elements {
                    self.mark_data(elem, marked);
                }
            }
            Data::Map(pairs) => {
                for (k, v) in pairs {
                    self.mark_data(k, marked);
                    self.mark_data(v, marked);
                }
            }
            _ => {}
        }
    }

    /// Sweep phase: remove unmarked boxes and update all references
    fn sweep(&mut self, marked: &HashSet<u32>) {
        // Build a mapping from old indices to new indices
        let mut index_map: Vec<Option<u32>> = vec![None; self.box_heap.len()];
        let mut new_heap = Vec::new();
        let mut new_idx = 0u32;

        for (old_idx, box_data) in self.box_heap.iter().enumerate() {
            if marked.contains(&(old_idx as u32)) {
                // This box is reachable, keep it
                index_map[old_idx] = Some(new_idx);
                new_heap.push(box_data.clone());
                new_idx += 1;
            }
            // Otherwise, this box is garbage and will be removed
        }

        // Replace the old heap with the compacted heap
        self.box_heap = new_heap;

        // Update all BoxRef references to point to new indices
        self.update_box_refs(&index_map);
    }

    /// Update all BoxRef references after compaction
    fn update_box_refs(&mut self, index_map: &[Option<u32>]) {
        // Update references in stack frames
        for frame in &mut self.stack {
            Self::update_data_vec(&mut frame.stack, index_map);
            Self::update_data_vec(&mut frame.locals, index_map);
            Self::update_data_vec(&mut frame.params, index_map);
        }

        // Update references in heap structs
        for struct_obj in &mut self.heap {
            Self::update_data_vec(&mut struct_obj.fields, index_map);
        }

        // Update references in box_heap itself (boxes can contain boxes)
        for box_data in &mut self.box_heap {
            Self::update_data(box_data, index_map);
        }
    }

    /// Update a vector of Data values with new box indices
    fn update_data_vec(data_vec: &mut [Data], index_map: &[Option<u32>]) {
        for data in data_vec {
            Self::update_data(data, index_map);
        }
    }

    /// Update a single Data value with new box index
    fn update_data(data: &mut Data, index_map: &[Option<u32>]) {
        match data {
            Data::BoxRef(old_idx) => {
                match index_map.get(*old_idx as usize) {
                    Some(Some(new_idx)) => {
                        *old_idx = *new_idx;
                    }
                    Some(None) => {
                        // This box was garbage collected, but we're trying to update a reference to it.
                        // This should never happen if mark phase is correct.
                        panic!(
                            "BUG: Attempted to update reference to garbage-collected box at index {}",
                            old_idx
                        );
                    }
                    None => {
                        // Index out of bounds - this should never happen
                        panic!(
                            "BUG: BoxRef index {} is out of bounds (heap size: {})",
                            old_idx,
                            index_map.len()
                        );
                    }
                }
            }
            Data::ProtoBoxRef {
                box_idx: old_idx,
                concrete_type_id: _concrete_type_id,
            } => match index_map.get(*old_idx as usize) {
                Some(Some(new_idx)) => {
                    *old_idx = *new_idx;
                }
                Some(None) => {
                    panic!(
                        "BUG: Attempted to update reference to garbage-collected proto box at index {}",
                        old_idx
                    );
                }
                None => {
                    panic!(
                        "BUG: ProtoBoxRef index {} is out of bounds (heap size: {})",
                        old_idx,
                        index_map.len()
                    );
                }
            },
            Data::Array(elements) => {
                Self::update_data_vec(elements, index_map);
            }
            Data::Some(inner) => {
                Self::update_data(inner, index_map);
            }
            Data::Closure { captures, .. } => {
                Self::update_data_vec(captures, index_map);
            }
            Data::Tuple(elements) => {
                Self::update_data_vec(elements, index_map);
            }
            Data::Map(pairs) => {
                for (k, v) in pairs.iter_mut() {
                    Self::update_data(k, index_map);
                    Self::update_data(v, index_map);
                }
            }
            _ => {}
        }
    }
}
