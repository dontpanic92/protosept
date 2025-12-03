pub mod ast;
pub mod bytecode;
pub mod errors;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod semantic;

use crate::errors::Proto7Error;

pub fn run_p7_code(
    contents: String,
    entrypoint: &str,
) -> Result<interpreter::context::Data, Proto7Error> {
    let mut lexer = lexer::Lexer::new(contents);
    let mut tokens = vec![];

    loop {
        let token = lexer.next_token();
        if token.token_type == lexer::TokenType::EOF {
            break;
        } else {
            tokens.push(token);
        }
    }

    let mut parser = parser::Parser::new(tokens);
    let statements = parser.parse()?;

    let mut codegen = bytecode::codegen::Generator::new();
    let module = codegen.generate(statements)?;

    let mut context = interpreter::context::Context::new();
    context.load_module(module);
    context.push_function(entrypoint, Vec::new());
    context.resume().unwrap();

    let result = context.stack[0].stack.pop().unwrap();
    Ok(result)
}
