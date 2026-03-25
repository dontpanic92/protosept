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
    ctx.register_host_function("string.contains".to_string(), string_contains);
    ctx.register_host_function("string.ends_with".to_string(), string_ends_with);
    ctx.register_host_function("string.repeat".to_string(), string_repeat);
    ctx.register_host_function("string.trim".to_string(), string_trim);
    ctx.register_host_function("string.trim_start".to_string(), string_trim_start);
    ctx.register_host_function("string.trim_end".to_string(), string_trim_end);
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
    ctx.register_host_function("array.insert".to_string(), array_insert);
    ctx.register_host_function("array.remove".to_string(), array_remove);
    ctx.register_host_function("array.index_of".to_string(), array_index_of);
    ctx.register_host_function("array.join".to_string(), array_join);
    ctx.register_host_function("array.map".to_string(), array_map);
    ctx.register_host_function("array.filter".to_string(), array_filter);
    ctx.register_host_function("array.reduce".to_string(), array_reduce);
    ctx.register_host_function("array.for_each".to_string(), array_for_each);
    ctx.register_host_function("array.find".to_string(), array_find);
    ctx.register_host_function("array.any".to_string(), array_any);
    ctx.register_host_function("array.all".to_string(), array_all);
    ctx.register_host_function("builtin.entry_script_dir".to_string(), builtin_entry_script_dir);
    ctx.register_host_function("builtin.min".to_string(), builtin_min);
    ctx.register_host_function("builtin.max".to_string(), builtin_max);
    ctx.register_host_function("builtin.clamp".to_string(), builtin_clamp);
    ctx.register_host_function("tuple.new".to_string(), tuple_new);
    ctx.register_host_function("tuple.index".to_string(), tuple_index);
    ctx.register_host_function("hashmap.new".to_string(), hashmap_new);
    ctx.register_host_function("hashmap.len".to_string(), hashmap_len);
    ctx.register_host_function("hashmap.get".to_string(), hashmap_get);
    ctx.register_host_function("hashmap.set".to_string(), hashmap_set);
    ctx.register_host_function("hashmap.remove".to_string(), hashmap_remove);
    ctx.register_host_function("hashmap.contains_key".to_string(), hashmap_contains_key);
    ctx.register_host_function("hashmap.keys".to_string(), hashmap_keys);
    ctx.register_host_function("hashmap.values".to_string(), hashmap_values);
    ctx.register_host_function("hashmap.index".to_string(), hashmap_index);
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
            let byte_len = s.len() as i64;
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
            let len = elements.len() as i64;
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
            let char_len = s.chars().count() as i64;
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
            let char_count = s.chars().count() as i64;
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
                Some(byte_pos) => s[..byte_pos].chars().count() as i64,
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

fn array_insert(ctx: &mut Context) -> ContextResult<()> {
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

fn array_remove(ctx: &mut Context) -> ContextResult<()> {
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

fn string_contains(ctx: &mut Context) -> ContextResult<()> {
    let needle_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, needle_val) {
        (Data::String(s), Data::String(needle)) => {
            let result = if s.contains(&needle) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.contains: invalid argument types".to_string(),
        )),
    }
}

fn string_ends_with(ctx: &mut Context) -> ContextResult<()> {
    let suffix_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, suffix_val) {
        (Data::String(s), Data::String(suffix)) => {
            let result = if s.ends_with(&suffix) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.ends_with: invalid argument types".to_string(),
        )),
    }
}

fn string_repeat(ctx: &mut Context) -> ContextResult<()> {
    let n_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, n_val) {
        (Data::String(s), Data::Int(n)) => {
            let result = if n <= 0 {
                String::new()
            } else {
                s.repeat(n as usize)
            };
            ctx.stack_frame_mut()?.stack.push(Data::String(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.repeat: invalid argument types".to_string(),
        )),
    }
}

fn array_index_of(ctx: &mut Context) -> ContextResult<()> {
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

fn builtin_min(ctx: &mut Context) -> ContextResult<()> {
    let b = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let a = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match (a, b) {
        (Data::Int(a), Data::Int(b)) => {
            ctx.stack_frame_mut()?.stack.push(Data::Int(a.min(b)));
            Ok(())
        }
        _ => Err(RuntimeError::Other("min: arguments must be int".to_string())),
    }
}

fn builtin_max(ctx: &mut Context) -> ContextResult<()> {
    let b = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let a = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match (a, b) {
        (Data::Int(a), Data::Int(b)) => {
            ctx.stack_frame_mut()?.stack.push(Data::Int(a.max(b)));
            Ok(())
        }
        _ => Err(RuntimeError::Other("max: arguments must be int".to_string())),
    }
}

fn builtin_clamp(ctx: &mut Context) -> ContextResult<()> {
    let hi = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let lo = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    let value = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match (value, lo, hi) {
        (Data::Int(v), Data::Int(lo), Data::Int(hi)) => {
            ctx.stack_frame_mut()?.stack.push(Data::Int(v.clamp(lo, hi)));
            Ok(())
        }
        _ => Err(RuntimeError::Other("clamp: arguments must be int".to_string())),
    }
}

fn string_trim(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim: expected string".to_string())),
    }
}

fn string_trim_start(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim_start().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim_start: expected string".to_string())),
    }
}

fn string_trim_end(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim_end().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim_end: expected string".to_string())),
    }
}

fn array_join(ctx: &mut Context) -> ContextResult<()> {
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

fn array_map(ctx: &mut Context) -> ContextResult<()> {
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

fn array_filter(ctx: &mut Context) -> ContextResult<()> {
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

fn array_reduce(ctx: &mut Context) -> ContextResult<()> {
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

fn array_for_each(ctx: &mut Context) -> ContextResult<()> {
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

fn array_find(ctx: &mut Context) -> ContextResult<()> {
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

fn array_any(ctx: &mut Context) -> ContextResult<()> {
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

fn array_all(ctx: &mut Context) -> ContextResult<()> {
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

fn tuple_new(ctx: &mut Context) -> ContextResult<()> {
    let element_count_data = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "tuple.new: missing element count".to_string(),
        ))?;

    let element_count = match element_count_data {
        Data::Int(count) => {
            if count < 2 {
                return Err(RuntimeError::Other(format!(
                    "tuple.new: element count must be >= 2, found {}",
                    count
                )));
            }
            count as u32
        }
        _ => {
            return Err(RuntimeError::Other(format!(
                "tuple.new: element count must be int, found {:?}",
                element_count_data
            )));
        }
    };

    let mut elements = Vec::new();
    for _ in 0..element_count {
        if let Some(elem) = ctx.stack_frame_mut()?.stack.pop() {
            elements.push(elem);
        } else {
            return Err(RuntimeError::StackUnderflow);
        }
    }
    elements.reverse();
    ctx.stack_frame_mut()?.stack.push(Data::Tuple(elements));
    Ok(())
}

fn tuple_index(ctx: &mut Context) -> ContextResult<()> {
    let index = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    let tuple = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (tuple, index) {
        (Data::Tuple(elements), Data::Int(idx)) => {
            if idx < 0 || (idx as usize) >= elements.len() {
                return Err(RuntimeError::Other(format!(
                    "Tuple index out of bounds: index {} for tuple of length {}",
                    idx,
                    elements.len()
                )));
            }
            let element = elements[idx as usize].clone();
            ctx.stack_frame_mut()?.stack.push(element);
            Ok(())
        }
        (Data::Tuple(_), _) => Err(RuntimeError::Other(
            "tuple.index: index must be an integer".to_string(),
        )),
        _ => Err(RuntimeError::Other(
            "tuple.index: first argument must be a tuple".to_string(),
        )),
    }
}

// ===== HashMap functions =====

/// Pop count and (count * 2) values from stack, build a Map.
/// Stack layout: [k0, v0, k1, v1, ..., kN, vN, count]
fn hashmap_new(ctx: &mut Context) -> ContextResult<()> {
    let count_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    let count = match count_val {
        Data::Int(n) => n as usize,
        _ => {
            return Err(RuntimeError::Other(
                "hashmap.new: expected integer count".to_string(),
            ))
        }
    };

    // Pop key-value pairs. They are on the stack in order:
    // bottom ... k0 v0 k1 v1 ... kN vN count
    // We already popped count, now pop 2*count values
    let mut flat = Vec::with_capacity(count * 2);
    for _ in 0..count * 2 {
        flat.push(
            ctx.stack_frame_mut()?
                .stack
                .pop()
                .ok_or(RuntimeError::StackUnderflow)?,
        );
    }
    // flat is in reverse stack order: [vN, kN, ..., v0, k0]
    flat.reverse();

    let mut pairs = Vec::with_capacity(count);
    for chunk in flat.chunks(2) {
        pairs.push((chunk[0].clone(), chunk[1].clone()));
    }

    ctx.stack_frame_mut()?.stack.push(Data::Map(pairs));
    Ok(())
}

fn hashmap_len(ctx: &mut Context) -> ContextResult<()> {
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::Int(pairs.len() as i64));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.len: expected map".to_string(),
        )),
    }
}

fn hashmap_get(ctx: &mut Context) -> ContextResult<()> {
    let key = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            let found = pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
            match found {
                Some(v) => ctx
                    .stack_frame_mut()?
                    .stack
                    .push(Data::Some(Box::new(v))),
                None => ctx.stack_frame_mut()?.stack.push(Data::Null),
            }
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.get: expected map".to_string(),
        )),
    }
}

fn hashmap_set(ctx: &mut Context) -> ContextResult<()> {
    let value = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let key = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
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
                Data::Map(pairs) => {
                    if let Some(entry) = pairs.iter_mut().find(|(k, _)| *k == key) {
                        entry.1 = value;
                    } else {
                        pairs.push((key, value));
                    }
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "hashmap.set: boxed value must be a map".to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Other(
            "hashmap.set: first argument must be a box reference".to_string(),
        )),
    }
}

fn hashmap_remove(ctx: &mut Context) -> ContextResult<()> {
    let key = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
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
                Data::Map(pairs) => {
                    let removed = pairs
                        .iter()
                        .position(|(k, _)| *k == key)
                        .map(|idx| pairs.remove(idx).1);
                    match removed {
                        Some(v) => ctx
                            .stack_frame_mut()?
                            .stack
                            .push(Data::Some(Box::new(v))),
                        None => ctx.stack_frame_mut()?.stack.push(Data::Null),
                    }
                    Ok(())
                }
                _ => Err(RuntimeError::Other(
                    "hashmap.remove: boxed value must be a map".to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Other(
            "hashmap.remove: first argument must be a box reference".to_string(),
        )),
    }
}

fn hashmap_contains_key(ctx: &mut Context) -> ContextResult<()> {
    let key = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            let found = pairs.iter().any(|(k, _)| *k == key);
            ctx.stack_frame_mut()?
                .stack
                .push(Data::Int(found as i64));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.contains_key: expected map".to_string(),
        )),
    }
}

fn hashmap_keys(ctx: &mut Context) -> ContextResult<()> {
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            let keys: Vec<Data> = pairs.into_iter().map(|(k, _)| k).collect();
            ctx.stack_frame_mut()?.stack.push(Data::Array(keys));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.keys: expected map".to_string(),
        )),
    }
}

fn hashmap_values(ctx: &mut Context) -> ContextResult<()> {
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            let values: Vec<Data> = pairs.into_iter().map(|(_, v)| v).collect();
            ctx.stack_frame_mut()?.stack.push(Data::Array(values));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.values: expected map".to_string(),
        )),
    }
}

/// Index into a map — used for map[key] syntax. Traps if key not found.
fn hashmap_index(ctx: &mut Context) -> ContextResult<()> {
    let key = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(pairs) => {
            let found = pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
            match found {
                Some(v) => {
                    ctx.stack_frame_mut()?.stack.push(v);
                    Ok(())
                }
                None => Err(RuntimeError::Other(format!(
                    "Key not found in HashMap: {:?}",
                    key
                ))),
            }
        }
        _ => Err(RuntimeError::Other(
            "hashmap.index: expected map".to_string(),
        )),
    }
}
