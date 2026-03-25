use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn tuple_new(ctx: &mut Context) -> ContextResult<()> {
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

pub(crate) fn tuple_index(ctx: &mut Context) -> ContextResult<()> {
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
