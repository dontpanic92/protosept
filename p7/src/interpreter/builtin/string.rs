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
            let mut result = String::with_capacity(a.len() + b.len());
            result.push_str(&a);
            result.push_str(&b);
            ctx.stack_frame_mut()?.stack.push(Data::string(result));
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
            let start_idx = start.max(0) as usize;
            let end_idx = end.max(0) as usize;
            let result = substring_by_char_range(&s, start_idx, end_idx);
            ctx.stack_frame_mut()?.stack.push(Data::string(result));
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
            match char_at(&s, idx as usize) {
                Some(ch) => {
                    ctx.stack_frame_mut()?
                        .stack
                        .push(Data::some(Data::string(ch)));
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

fn substring_by_char_range(s: &str, start: usize, end: usize) -> &str {
    if start >= end {
        return "";
    }

    let mut start_byte = None;
    let mut end_byte = None;
    let mut char_count = 0usize;

    for (char_idx, (byte_idx, _)) in s.char_indices().enumerate() {
        char_count = char_idx + 1;
        if char_idx == start {
            start_byte = Some(byte_idx);
        }
        if char_idx == end {
            end_byte = Some(byte_idx);
            break;
        }
    }

    let Some(start_byte) = start_byte else {
        return "";
    };
    let end_byte = end_byte.unwrap_or_else(|| {
        if end >= char_count {
            s.len()
        } else {
            start_byte
        }
    });

    &s[start_byte..end_byte]
}

fn char_at(s: &str, index: usize) -> Option<&str> {
    let (start, ch) = s.char_indices().nth(index)?;
    let end = start + ch.len_utf8();
    Some(&s[start..end])
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
            let parts: Vec<Data> = s.split(delim.as_ref()).map(Data::string).collect();
            ctx.stack_frame_mut()?.stack.push(Data::array(parts));
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
            let result = match s.find(needle.as_ref()) {
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
            let result = if s.starts_with(prefix.as_ref()) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.starts_with: invalid argument types".to_string(),
        )),
    }
}

pub(crate) fn string_contains(ctx: &mut Context) -> ContextResult<()> {
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
            let result = if s.contains(needle.as_ref()) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.contains: invalid argument types".to_string(),
        )),
    }
}

pub(crate) fn string_ends_with(ctx: &mut Context) -> ContextResult<()> {
    let suffix_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, suffix_val) {
        (Data::String(s), Data::String(suffix)) => {
            let result = if s.ends_with(suffix.as_ref()) { 1 } else { 0 };
            ctx.stack_frame_mut()?.stack.push(Data::Int(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.ends_with: invalid argument types".to_string(),
        )),
    }
}

pub(crate) fn string_repeat(ctx: &mut Context) -> ContextResult<()> {
    let n_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;

    match (self_val, n_val) {
        (Data::String(s), Data::Int(n)) => {
            let result = if n <= 0 {
                String::new()
            } else {
                s.repeat(n as usize)
            };
            ctx.stack_frame_mut()?.stack.push(Data::string(result));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.repeat: invalid argument types".to_string(),
        )),
    }
}

pub(crate) fn string_trim(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?.stack.push(Data::string(s.trim()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.trim: expected string".to_string(),
        )),
    }
}

pub(crate) fn string_trim_start(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::string(s.trim_start()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.trim_start: expected string".to_string(),
        )),
    }
}

pub(crate) fn string_trim_end(ctx: &mut Context) -> ContextResult<()> {
    let self_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::StackUnderflow)?;
    match self_val {
        Data::String(s) => {
            ctx.stack_frame_mut()?
                .stack
                .push(Data::string(s.trim_end()));
            Ok(())
        }
        _ => Err(RuntimeError::Other(
            "string.trim_end: expected string".to_string(),
        )),
    }
}
