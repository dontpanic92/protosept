use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

/// Pop count and (count * 2) values from stack, build a Map.
/// Stack layout: [k0, v0, k1, v1, ..., kN, vN, count]
pub(crate) fn hashmap_new(ctx: &mut Context) -> ContextResult<()> {
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
            ));
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

pub(crate) fn hashmap_len(ctx: &mut Context) -> ContextResult<()> {
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
        _ => Err(RuntimeError::Other("hashmap.len: expected map".to_string())),
    }
}

pub(crate) fn hashmap_get(ctx: &mut Context) -> ContextResult<()> {
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
            let found = pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone());
            match found {
                Some(v) => ctx.stack_frame_mut()?.stack.push(Data::Some(Box::new(v))),
                None => ctx.stack_frame_mut()?.stack.push(Data::Null),
            }
            Ok(())
        }
        _ => Err(RuntimeError::Other("hashmap.get: expected map".to_string())),
    }
}

pub(crate) fn hashmap_set(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn hashmap_remove(ctx: &mut Context) -> ContextResult<()> {
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
                        Some(v) => ctx.stack_frame_mut()?.stack.push(Data::Some(Box::new(v))),
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

pub(crate) fn hashmap_contains_key(ctx: &mut Context) -> ContextResult<()> {
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
            ctx.stack_frame_mut()?.stack.push(Data::Int(found as i64));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.contains_key: expected map".to_string(),
        )),
    }
}

pub(crate) fn hashmap_keys(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn hashmap_values(ctx: &mut Context) -> ContextResult<()> {
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
pub(crate) fn hashmap_index(ctx: &mut Context) -> ContextResult<()> {
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
            let found = pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone());
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
