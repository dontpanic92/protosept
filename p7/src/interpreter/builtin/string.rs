use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn string_len_bytes(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_display(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_concat(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_len_chars(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_substring(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_char_at(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_split(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_index_of(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_starts_with(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_contains(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_ends_with(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_repeat(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn string_trim(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim: expected string".to_string())),
    }
}

pub(crate) fn string_trim_start(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim_start().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim_start: expected string".to_string())),
    }
}

pub(crate) fn string_trim_end(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::String(s.trim_end().to_string()));
            Ok(())
        }
        _ => Err(RuntimeError::Other("string.trim_end: expected string".to_string())),
    }
}
