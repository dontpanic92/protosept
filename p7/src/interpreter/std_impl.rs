use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn register_std_functions(ctx: &mut Context) {
    ctx.register_host_function("std.io.println".to_string(), std_io_println);
}

fn std_io_println(ctx: &mut Context) -> ContextResult<()> {
    let string_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "std.io.println: missing string argument".to_string(),
        ))?;

    match string_val {
        Data::String(s) => {
            println!("{}", s);
            // Push unit value (int 0) as return value
            ctx.stack_frame_mut()?.stack.push(Data::Int(0));
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "std.io.println expected string, found {:?}",
            string_val
        ))),
    }
}
