use std::collections::HashSet;

use super::Context;
use super::data::Data;
use crate::errors::RuntimeError;

impl Context {
    /// Mark-and-sweep garbage collector for the box heap.
    ///
    /// Storage is a stable-handle slab ([`super::BoxHeap`]); the sweep
    /// frees unmarked slots in place rather than compacting. Each freed
    /// slot's generation is bumped, so any outstanding `Data::*BoxRef`
    /// (in Rust locals or anywhere else GC can't reach) fails fast on
    /// dereference instead of silently aliasing the slot's next occupant.
    pub fn collect_garbage(&mut self) -> Result<(), RuntimeError> {
        // Mark phase: identify all reachable boxes
        let mut marked = HashSet::new();
        self.mark_reachable(&mut marked);

        // Sweep phase: free unmarked slots and run finalizers
        self.sweep(&marked)
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

        for root in &self.external_roots {
            if let Some(data) = root {
                self.mark_data(data, marked);
            }
        }
    }

    /// Recursively mark a data value and any boxes it references
    fn mark_data(&self, data: &Data, marked: &mut HashSet<u32>) {
        match data {
            Data::BoxRef { idx, .. } | Data::ProtoBoxRef { box_idx: idx, .. } => {
                // If we haven't marked this box yet, mark it and recursively mark its contents.
                // We use `get_unchecked` here: the mark phase walks live reachable references,
                // and a stale handle reachable from a dead struct would simply not find a slot.
                if marked.insert(*idx)
                    && let Some(box_data) = self.box_heap.get_unchecked(*idx)
                {
                    self.mark_data(box_data, marked);
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
                for elem in elements.iter() {
                    self.mark_data(elem, marked);
                }
            }
            Data::Some(inner) => {
                self.mark_data(inner, marked);
            }
            Data::Closure { captures, .. } => {
                for capture in captures.iter() {
                    self.mark_data(capture, marked);
                }
            }
            Data::Tuple(elements) => {
                for elem in elements.iter() {
                    self.mark_data(elem, marked);
                }
            }
            Data::Map(map) => {
                for entry in map.entries() {
                    self.mark_data(entry.key(), marked);
                    self.mark_data(entry.value(), marked);
                }
            }
            _ => {}
        }
    }

    /// Sweep phase: free unmarked slots in place. Owned `Data::Foreign`
    /// cells trigger their proto's registered finalizer host fn with
    /// type_tag and handle metadata.
    ///
    /// Slot indices are stable; freeing only bumps the generation and
    /// pushes the slot onto the free list for future allocations.
    /// References elsewhere in the graph need no rewriting.
    fn sweep(&mut self, marked: &HashSet<u32>) -> Result<(), RuntimeError> {
        let mut pending_finalizers: Vec<(String, String, i64)> = Vec::new();

        // Walk the live slots and identify those not in the mark set.
        let to_free: Vec<u32> = self
            .box_heap
            .iter_live()
            .filter_map(|(idx, _)| {
                if marked.contains(&idx) {
                    None
                } else {
                    Some(idx)
                }
            })
            .collect();

        for idx in to_free {
            // Capture finalizer metadata before freeing the slot.
            if let Some(Data::Foreign {
                type_tag,
                handle,
                owned: true,
                ..
            }) = self.box_heap.get_unchecked(idx)
                && let Some(reg) = self.foreign_types.get(type_tag.as_str())
                && let Some(name) = &reg.finalizer
            {
                pending_finalizers.push((name.clone(), type_tag.clone(), *handle));
            }
            self.box_heap.free(idx);
        }

        // Fire finalizers in collection order. Each finalizer is an
        // ordinary host fn registered via `register_host_function`; it
        // receives type_tag at the top of the stack and handle below it.
        for (name, type_tag, handle) in pending_finalizers {
            let host_fn = self.host_functions.get(&name).cloned().ok_or_else(|| {
                RuntimeError::Other(format!(
                    "Foreign finalizer '{}' for type_tag '{}' is not registered",
                    name, type_tag
                ))
            })?;
            let stack_len = {
                let frame = self.stack.last_mut().ok_or(RuntimeError::NoStackFrame)?;
                let stack_len = frame.stack.len();
                frame.stack.push(Data::Int(handle));
                frame.stack.push(Data::string(type_tag.clone()));
                stack_len
            };
            if let Err(err) = host_fn(self) {
                if let Some(frame) = self.stack.last_mut() {
                    frame.stack.truncate(stack_len);
                }
                return Err(RuntimeError::Other(format!(
                    "Foreign finalizer '{}' for type_tag '{}' failed: {}",
                    name, type_tag, err
                )));
            }
        }
        Ok(())
    }
}
