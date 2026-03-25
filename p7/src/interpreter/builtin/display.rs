use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn builtin_entry_script_dir(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn display_int(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn display_float(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn display_bool(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn display_char(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn display_unit(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn builtin_min(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn builtin_max(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn builtin_clamp(ctx: &mut Context) -> ContextResult<()> {
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
