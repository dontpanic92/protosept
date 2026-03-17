use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn register_builtin_functions(ctx: &mut Context) {
    ctx.register_host_function("string.len_bytes".to_string(), string_len_bytes);
    ctx.register_host_function("string.display".to_string(), string_display);
    ctx.register_host_function("string.concat".to_string(), string_concat);
    ctx.register_host_function("string.len_chars".to_string(), string_len_chars);
    ctx.register_host_function("string.substring".to_string(), string_substring);
    ctx.register_host_function("string.char_at".to_string(), string_char_at);
    ctx.register_host_function("string.split".to_string(), string_split);
    ctx.register_host_function("string.index_of".to_string(), string_index_of);
    ctx.register_host_function("string.starts_with".to_string(), string_starts_with);
    ctx.register_host_function("display.int".to_string(), display_int);
    ctx.register_host_function("display.float".to_string(), display_float);
    ctx.register_host_function("display.bool".to_string(), display_bool);
    ctx.register_host_function("display.char".to_string(), display_char);
    ctx.register_host_function("display.unit".to_string(), display_unit);
    ctx.register_host_function("array.new".to_string(), array_new);
    ctx.register_host_function("array.index".to_string(), array_index);
    ctx.register_host_function("array.get".to_string(), array_get);
    ctx.register_host_function("array.len".to_string(), array_len);
    ctx.register_host_function("array.slice".to_string(), array_slice);
    ctx.register_host_function("array.push".to_string(), array_push);
    ctx.register_host_function("array.clear".to_string(), array_clear);
    ctx.register_host_function("array.pop".to_string(), array_pop);
    ctx.register_host_function("array.set".to_string(), array_set);
    ctx.register_host_function("builtin.entry_script_dir".to_string(), builtin_entry_script_dir);
}

fn builtin_entry_script_dir(ctx: &mut Context) -> ContextResult<()> {
    match ctx.script_dir() {
        Some(dir) => {
            let dir_string = dir.to_string();
            ctx.stack_frame_mut()?
                .stack
                .push(Data::Some(Box::new(Data::String(dir_string))));
        }
        None => {
            ctx.stack_frame_mut()?.stack.push(Data::Null);
        }
    }
    Ok(())
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

fn string_display(ctx: &mut Context) -> ContextResult<()> {
    let string_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "string.display: missing self parameter".to_string(),
        ))?;

    match string_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "string.display expected string, found {:?}",
            string_val
        ))),
    }
}

fn string_concat(ctx: &mut Context) -> ContextResult<()> {
    let other_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "string.concat: missing other parameter".to_string(),
        ))?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "string.concat: missing self parameter".to_string(),
        ))?;

    match (self_val, other_val) {
        (Data::String(a), Data::String(b)) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String(format!("{}{}", a, b)));
            Ok(())
        }
        (Data::String(_), other) => Err(RuntimeError::Other(format!(
            "string.concat expected string, found {:?}",
            other
        ))),
        (other, _) => Err(RuntimeError::Other(format!(
            "string.concat expected string, found {:?}",
            other
        ))),
    }
}

fn display_int(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "display.int: missing value".to_string(),
        ))?;

    match value {
        Data::Int(v) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String(v.to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "display.int expected int, found {:?}",
            value
        ))),
    }
}

fn display_float(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "display.float: missing value".to_string(),
        ))?;

    match value {
        Data::Float(v) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String(v.to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "display.float expected float, found {:?}",
            value
        ))),
    }
}

fn display_bool(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "display.bool: missing value".to_string(),
        ))?;

    match value {
        Data::Int(v) => {
            let text = if v == 0 { "false" } else { "true" };
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String(text.to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "display.bool expected bool, found {:?}",
            value
        ))),
    }
}

fn display_char(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "display.char: missing value".to_string(),
        ))?;

    match value {
        Data::Int(v) => {
            let ch = char::from_u32(v as u32).ok_or_else(|| {
                RuntimeError::Other(format!(
                    "display.char expected valid unicode scalar, found {}",
                    v
                ))
            })?;
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String(ch.to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "display.char expected char, found {:?}",
            value
        ))),
    }
}

fn display_unit(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "display.unit: missing value".to_string(),
        ))?;

    match value {
        Data::Int(_) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::String("()".to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "display.unit expected unit, found {:?}",
            value
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
        (Data::Array(_), _) => Err(RuntimeError::Other(
            "array.index: index must be an integer".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "array.index: first argument must be an array".to_string(),
        )),
    }
}

fn array_get(ctx: &mut Context) -> ContextResult<()> {
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

fn array_pop(ctx: &mut Context) -> ContextResult<()> {
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

fn array_set(ctx: &mut Context) -> ContextResult<()> {
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

fn string_len_chars(ctx: &mut Context) -> ContextResult<()> {
    let string_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "string.len_chars: missing self parameter".to_string(),
        ))?;

    match string_val {
        Data::String(s) => {
            let char_len = s.chars().count() as i32;
            ctx.stack_frame_mut()?.stack.push(Data::Int(char_len));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "string.len_chars expected string, found {:?}",
            string_val
        ))),
    }
}

fn string_substring(ctx: &mut Context) -> ContextResult<()> {
    let end_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let start_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, start_val, end_val) {
        (Data::String(s), Data::Int(start), Data::Int(end)) => {
            let char_count = s.chars().count() as i32;
            let clamped_start = start.max(0).min(char_count) as usize;
            let clamped_end = end.max(0).min(char_count) as usize;

            let result: String = if clamped_start >= clamped_end {
                String::new()
            } else {
                s.chars().skip(clamped_start).take(clamped_end - clamped_start).collect()
            };
            ctx.stack_frame_mut()?.stack.push(Data::String(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.substring: invalid argument types".to_string(),
        )),
    }
}

fn string_char_at(ctx: &mut Context) -> ContextResult<()> {
    let index_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, index_val) {
        (Data::String(s), Data::Int(idx)) => {
            if idx < 0 {
                ctx.stack_frame_mut()?.stack.push(Data::Null);
                return Ok(());
            }
            match s.chars().nth(idx as usize) {
                Some(ch) => {
                    ctx.stack_frame_mut()?
                        .stack
                        .push(Data::Some(Box::new(Data::String(ch.to_string()))));
                }
                None => {
                    ctx.stack_frame_mut()?.stack.push(Data::Null);
                }
            }
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.char_at: invalid argument types".to_string(),
        )),
    }
}

fn string_split(ctx: &mut Context) -> ContextResult<()> {
    let delim_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, delim_val) {
        (Data::String(s), Data::String(delim)) => {
            let parts: Vec<Data> = s
                .split(&delim)
                .map(|part| Data::String(part.to_string()))
                .collect();
            ctx.stack_frame_mut()?.stack.push(Data::Array(parts));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.split: invalid argument types".to_string(),
        )),
    }
}

fn string_index_of(ctx: &mut Context) -> ContextResult<()> {
    let needle_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, needle_val) {
        (Data::String(s), Data::String(needle)) => {
            // Find byte offset, then convert to char index
            let result = match s.find(&needle) {
                Some(byte_pos) => s[..byte_pos].chars().count() as i32,
                None => -1,
            };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.index_of: invalid argument types".to_string(),
        )),
    }
}

fn string_starts_with(ctx: &mut Context) -> ContextResult<()> {
    let prefix_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, prefix_val) {
        (Data::String(s), Data::String(prefix)) => {
            let result = if s.starts_with(&prefix) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.starts_with: invalid argument types".to_string(),
        )),
    }
}
