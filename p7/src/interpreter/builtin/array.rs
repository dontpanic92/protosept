use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn array_new(ctx: &mut Context) -> ContextResult<()> {
    // Pop element count from stack
    let element_count_data = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "array.new: missing element count".to_string(),
        ))?;

    let element_count = match element_count_data {
        Data::Int(count) => {
            if count < 0 {
                return Err(RuntimeError::Other(format!(
                    "array.new: element count must be non-negative, found {}",
                    count
                )));
            }
            count as u32
        }
        _ => {
            return Err(RuntimeError::Other(format!(
                "array.new: element count must be int, found {:?}",
                element_count_data
            )));
        }
    };

    // Pop element_count values from stack and create an array
    let mut elements = Vec::new();
    for _ in 0..element_count {
        if let Some(elem) = ctx.stack_frame_mut()?.stack.pop() {
            elements.push(elem);
        } else {
            return Err(RuntimeError::StackUnderflow);
        }
    }
    // Elements were popped in reverse order, so reverse them
    elements.reverse();
    ctx.stack_frame_mut()?.stack.push(Data::Array(elements));
    Ok(())
}

pub(crate) fn array_index(ctx: &mut Context) -> ContextResult<()> {
    // Pop index from stack
    let index = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop array from stack
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (array, index) {
        (Data::Array(elements), Data::Int(idx)) => {
            // Check for negative index
            if idx < 0 {
                return Err(RuntimeError::Other(format!(
                    "Array index out of bounds: negative index {}",
                    idx
                )));
            }

            // Check bounds
            if (idx as usize) >= elements.len() {
                return Err(RuntimeError::Other(format!(
                    "Array index out of bounds: index {} >= length {}",
                    idx,
                    elements.len()
                )));
            }

            // Push element at index
            let element = elements[idx as usize].clone();
            ctx.stack_frame_mut()?.stack.push(element);
            Ok(())
        }
        (Data::Array(_), _) => Err(RuntimeError::Other(
            "array.index: index must be an integer".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "array.index: first argument must be an array".to_string(),
        )),
    }
}

pub(crate) fn array_get(ctx: &mut Context) -> ContextResult<()> {
    // Pop index from stack
    let index = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop array from stack
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (array, index) {
        (Data::Array(elements), Data::Int(idx)) => {
            if idx < 0 || (idx as usize) >= elements.len() {
                ctx.stack_frame_mut()?.stack.push(Data::Null);
                return Ok(());
            }

            let element = elements[idx as usize].clone();
            ctx.stack_frame_mut()?
                .stack
                .push(Data::Some(Box::new(element)));
            Ok(())
        }
        (Data::Array(_), _) => Err(RuntimeError::Other(
            "array.get: index must be an integer".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "array.get: first argument must be an array".to_string(),
        )),
    }
}

pub(crate) fn array_len(ctx: &mut Context) -> ContextResult<()> {
    // Pop array from stack (self parameter)
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let len = elements.len() as i64;
            ctx.stack_frame_mut()?.stack.push(Data::Int(len));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "array.len: first argument must be an array".to_string(),
        )),
    }
}

pub(crate) fn array_slice(ctx: &mut Context) -> ContextResult<()> {
    // Pop end index from stack
    let end = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop start index from stack
    let start = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop array from stack
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (array, start, end) {
        (Data::Array(elements), Data::Int(start_idx), Data::Int(end_idx)) => {
            let len = elements.len() as i64;

            // Clamp start and end indices
            let clamped_start = start_idx.max(0).min(len) as usize;
            let clamped_end = end_idx.max(0).min(len) as usize;

            // Create slice (empty if start >= end)
            let sliced_elements = if clamped_start >= clamped_end {
                Vec::new()
            } else {
                elements[clamped_start..clamped_end].to_vec()
            };

            ctx.stack_frame_mut()?
                .stack
                .push(Data::Array(sliced_elements));
            Ok(())
        }
        (Data::Array(_), _, _) => Err(RuntimeError::Other(
            "array.slice: start and end indices must be integers".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "array.slice: first argument must be an array".to_string(),
        )),
    }
}

pub(crate) fn array_push(ctx: &mut Context) -> ContextResult<()> {
    // Pop element to push from stack
    let elem = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop box reference from stack
    let box_ref = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match box_ref {
        Data::BoxRef(box_idx) => {
            // Get the boxed array
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;

            // Ensure it's an array
            match boxed_data {
                Data::Array(elements) => {
                    elements.push(elem);
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "array.push: boxed value must be an array".to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Other(
            "array.push: first argument must be a box reference".to_string(),
        )),
    }
}

pub(crate) fn array_clear(ctx: &mut Context) -> ContextResult<()> {
    // Pop box reference from stack
    let box_ref = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match box_ref {
        Data::BoxRef(box_idx) => {
            // Get the boxed array
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;

            // Ensure it's an array and clear it
            match boxed_data {
                Data::Array(elements) => {
                    elements.clear();
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "array.clear: boxed value must be an array".to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Other(
            "array.clear: first argument must be a box reference".to_string(),
        )),
    }
}

pub(crate) fn array_pop(ctx: &mut Context) -> ContextResult<()> {
    // Pop box reference from stack
    let box_ref = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match box_ref {
        Data::BoxRef(box_idx) => {
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;

            match boxed_data {
                Data::Array(elements) => {
                    let value = elements.pop();
                    match value {
                        Some(elem) => ctx
                            .stack_frame_mut()?
                            .stack
                            .push(Data::Some(Box::new(elem))),
                        None => ctx.stack_frame_mut()?.stack.push(Data::Null),
                    }
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "array.pop: boxed value must be an array".to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Other(
            "array.pop: first argument must be a box reference".to_string(),
        )),
    }
}

pub(crate) fn array_set(ctx: &mut Context) -> ContextResult<()> {
    // Pop element to set from stack
    let elem = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop index from stack
    let index = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    // Pop box reference from stack
    let box_ref = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (box_ref, index) {
        (Data::BoxRef(box_idx), Data::Int(idx)) => {
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;

            match boxed_data {
                Data::Array(elements) => {
                    if idx < 0 || (idx as usize) >= elements.len() {
                        ctx.stack_frame_mut()?.stack.push(Data::Null);
                        return Ok(());
                    }

                    let old = std::mem::replace(&mut elements[idx as usize], elem);
                    ctx.stack_frame_mut()?.stack.push(Data::Some(Box::new(old)));
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "array.set: boxed value must be an array".to_string(),
                )),
            }
        }
        (Data::BoxRef(_), _) => Err(RuntimeError::Other(
            "array.set: index must be an integer".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "array.set: first argument must be a box reference".to_string(),
        )),
    }
}

pub(crate) fn array_insert(ctx: &mut Context) -> ContextResult<()> {
    let elem = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let index = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let box_ref = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (box_ref, index) {
        (Data::BoxRef(box_idx), Data::Int(idx)) => {
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;
            match boxed_data {
                Data::Array(elements) => {
                    let len = elements.len() as i64;
                    let clamped = idx.max(0).min(len) as usize;
                    elements.insert(clamped, elem);
                    Ok(())
                }
                _ => Err(RuntimeError::Other("array.insert: boxed value must be an array".to_string())),
            }
        }
        _ => Err(RuntimeError::Other("array.insert: invalid arguments".to_string())),
    }
}

pub(crate) fn array_remove(ctx: &mut Context) -> ContextResult<()> {
    let index = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let box_ref = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (box_ref, index) {
        (Data::BoxRef(box_idx), Data::Int(idx)) => {
            let boxed_data = ctx.box_heap.get_mut(box_idx as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid box reference: {}", box_idx))
            })?;
            match boxed_data {
                Data::Array(elements) => {
                    if idx < 0 || (idx as usize) >= elements.len() {
                        ctx.stack_frame_mut()?.stack.push(Data::Null);
                        return Ok(());
                    }
                    let removed = elements.remove(idx as usize);
                    ctx.stack_frame_mut()?.stack.push(Data::Some(Box::new(removed)));
                    Ok(())
                }
                _ => Err(RuntimeError::Other("array.remove: boxed value must be an array".to_string())),
            }
        }
        _ => Err(RuntimeError::Other("array.remove: invalid arguments".to_string())),
    }
}

pub(crate) fn array_index_of(ctx: &mut Context) -> ContextResult<()> {
    // Pop elem (search target), then self (array)
    let elem = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let mut found = -1i64;
            for (i, e) in elements.iter().enumerate() {
                if *e == elem {
                    found = i as i64;
                    break;
                }
            }
            ctx.stack_frame_mut()?.stack.push(Data::Int(found));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "array.index_of: first argument must be an array".to_string(),
        )),
    }
}

pub(crate) fn array_join(ctx: &mut Context) -> ContextResult<()> {
    let sep = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (array, sep) {
        (Data::Array(elements), Data::String(separator)) => {
            let strs: Vec<String> = elements.into_iter().map(|e| {
                match e {
                    Data::String(s) => s,
                    _ => String::new(),
                }
            }).collect();
            ctx.stack_frame_mut()?.stack.push(Data::String(strs.join(&separator)));
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.join: expected (array<string>, string)".to_string())),
    }
}

// --- Higher-order array functions ---

pub(crate) fn array_map(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let mut results = Vec::with_capacity(elements.len());
            for elem in elements {
                let result = ctx.call_closure(&closure, vec![elem])?;
                results.push(result);
            }
            ctx.stack_frame_mut()?.stack.push(Data::Array(results));
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.map: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_filter(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let mut results = Vec::new();
            for elem in elements {
                let result = ctx.call_closure(&closure, vec![elem.clone()])?;
                if result == Data::Int(1) {
                    results.push(elem);
                }
            }
            ctx.stack_frame_mut()?.stack.push(Data::Array(results));
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.filter: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_reduce(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let init = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let mut acc = init;
            for elem in elements {
                acc = ctx.call_closure(&closure, vec![acc, elem])?;
            }
            ctx.stack_frame_mut()?.stack.push(acc);
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.reduce: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_for_each(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            for elem in elements {
                ctx.call_closure_void(&closure, vec![elem])?;
            }
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.for_each: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_find(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            for elem in elements {
                let result = ctx.call_closure(&closure, vec![elem.clone()])?;
                if result == Data::Int(1) {
                    ctx.stack_frame_mut()?.stack.push(Data::Some(Box::new(elem)));
                    return Ok(());
                }
            }
            ctx.stack_frame_mut()?.stack.push(Data::Null);
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.find: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_any(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            for elem in elements {
                let result = ctx.call_closure(&closure, vec![elem])?;
                if result == Data::Int(1) {
                    ctx.stack_frame_mut()?.stack.push(Data::Int(1));
                    return Ok(());
                }
            }
            ctx.stack_frame_mut()?.stack.push(Data::Int(0));
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.any: first argument must be an array".to_string())),
    }
}

pub(crate) fn array_all(ctx: &mut Context) -> ContextResult<()> {
    let closure = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let array = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            for elem in elements {
                let result = ctx.call_closure(&closure, vec![elem])?;
                if result != Data::Int(1) {
                    ctx.stack_frame_mut()?.stack.push(Data::Int(0));
                    return Ok(());
                }
            }
            ctx.stack_frame_mut()?.stack.push(Data::Int(1));
            Ok(())
        }
        _ => Err(RuntimeError::Other("array.all: first argument must be an array".to_string())),
    }
}
