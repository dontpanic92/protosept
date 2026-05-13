use crate::{
    errors::RuntimeError,
    interpreter::context::{Context, ContextResult, Data},
};

pub(crate) fn register_std_functions(ctx: &mut Context) {
    ctx.register_host_function("std.io.println".to_string(), std_io_println);
    ctx.register_host_function("std.io.read_file".to_string(), std_io_read_file);
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

fn std_io_read_file(ctx: &mut Context) -> ContextResult<()> {
    let path_val = ctx
        .stack_frame_mut()?
        .stack
        .pop()
        .ok_or(RuntimeError::Other(
            "std.io.read_file: missing path argument".to_string(),
        ))?;

    match path_val {
        Data::String(path) => {
            let read_result = std::fs::read_to_string(path.as_ref()).or_else(|err| {
                #[cfg(windows)]
                {
                    if path.starts_with(r"\\?\") && path.contains('/') {
                        return std::fs::read_to_string(path.replace('/', r"\"));
                    }
                }
                Err(err)
            });

            match read_result {
                Ok(contents) => {
                    ctx.stack_frame_mut()?
                        .stack
                        .push(Data::some(Data::string(contents)));
                }
                Err(_) => {
                    ctx.stack_frame_mut()?.stack.push(Data::Null);
                }
            }
            Ok(())
        }
        _ => Err(RuntimeError::Other(format!(
            "std.io.read_file expected string, found {:?}",
            path_val
        ))),
    }
}
