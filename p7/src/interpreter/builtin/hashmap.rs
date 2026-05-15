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

    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
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
        pairs.push((key, value));
    }
    pairs.reverse();

    ctx.stack_frame_mut()?.stack.push(Data::try_map(pairs)?);
    Ok(())
}

pub(crate) fn hashmap_len(ctx: &mut Context) -> ContextResult<()> {
    let map_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match map_val {
        Data::Map(map) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::Int(map.len() as i64));
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
        Data::Map(map) => {
            match map.get(&key)?.cloned() {
                Some(v) => ctx.stack_frame_mut()?.stack.push(Data::some(v)),
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
        Data::BoxRef { idx: box_idx, generation } => {
            let boxed_data = ctx.box_heap.get_mut(box_idx, generation)?;
            match boxed_data {
                Data::Map(map) => {
                    std::rc::Rc::make_mut(map).insert(key, value)?;
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
        Data::BoxRef { idx: box_idx, generation } => {
            let boxed_data = ctx.box_heap.get_mut(box_idx, generation)?;
            match boxed_data {
                Data::Map(map) => {
                    match std::rc::Rc::make_mut(map).remove(&key)? {
                        Some(v) => ctx.stack_frame_mut()?.stack.push(Data::some(v)),
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
        Data::Map(map) => {
            let found = map.contains_key(&key)?;
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
        Data::Map(map) => {
            ctx.stack_frame_mut()?.stack.push(Data::array(map.keys()));
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
        Data::Map(map) => {
            ctx.stack_frame_mut()?.stack.push(Data::array(map.values()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "hashmap.values: expected map".to_string(),
        )),
    }
}

/// Index into a map �?used for map[key] syntax. Traps if key not found.
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
        Data::Map(map) => {
            match map.get(&key)?.cloned() {
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
