use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn register_builtin_functions(ctx: &mut Context) {
    ctx.register_host_function("string.len_bytes".to_string(), string_len_bytes);
    ctx.register_host_function("array.new".to_string(), array_new);
    ctx.register_host_function("array.index".to_string(), array_index);
    ctx.register_host_function("array.len".to_string(), array_len);
    ctx.register_host_function("array.slice".to_string(), array_slice);
    ctx.register_host_function("array.push".to_string(), array_push);
    ctx.register_host_function("array.clear".to_string(), array_clear);
}

fn string_len_bytes(ctx: &mut Context) -> ContextResult<()> {
    // The self parameter is passed as param 0 (it's a ref<string>, which is the string value itself)
    let string_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "string.len_bytes: missing self parameter".to_string(),
        ))?
        .clone();

    match string_val {
        Data::String(s) => {
            let byte_len = s.len() as i32;
            ctx.stack_frame_mut()?.stack.push(Data::Int(byte_len));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "string.len_bytes expected string, found {:?}",
            string_val
        ))),
    }
}

fn array_new(ctx: &mut Context) -> ContextResult<()> {
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

fn array_index(ctx: &mut Context) -> ContextResult<()> {
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
        (Data::Array(_), _) => {
            Err(RuntimeError::Other(
                "array.index: index must be an integer".to_string(),
            ))
        }
        _ => {
            Err(RuntimeError::Other(
                "array.index: first argument must be an array".to_string(),
            ))
        }
    }
}

fn array_len(ctx: &mut Context) -> ContextResult<()> {
    // Pop array from stack (self parameter)
    let array = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match array {
        Data::Array(elements) => {
            let len = elements.len() as i32;
            ctx.stack_frame_mut()?.stack.push(Data::Int(len));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "array.len: first argument must be an array".to_string(),
        )),
    }
}

fn array_slice(ctx: &mut Context) -> ContextResult<()> {
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
            let len = elements.len() as i32;

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

fn array_push(ctx: &mut Context) -> ContextResult<()> {
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
            let boxed_data = ctx
                .box_heap
                .get_mut(box_idx as usize)
                .ok_or_else(|| {
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

fn array_clear(ctx: &mut Context) -> ContextResult<()> {
    // Pop box reference from stack
    let box_ref = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match box_ref {
        Data::BoxRef(box_idx) => {
            // Get the boxed array
            let boxed_data = ctx
                .box_heap
                .get_mut(box_idx as usize)
                .ok_or_else(|| {
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
