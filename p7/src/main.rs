use std::{error::Error, fs, path::Path};

fn test_parser_with_file() -> Result<(), Box<dyn Error>> {
    let file_path = Path::new("tests/test2.p7");
    let contents = fs::read_to_string(file_path)?;

    let mut lexer = p7::lexer::Lexer::new(contents);
    let mut tokens = vec![];
    
    loop {
        let token = lexer.next_token();
        if token.token_type == p7::lexer::TokenType::EOF {
            break;
        } else {
            tokens.push(token);
        }
    }

    let mut parser = p7::parser::Parser::new(tokens);
    let statements = parser.parse()?;

    let mut codegen = p7::bytecode::codegen::Generator::new();
    let module = codegen.generate(statements)?;

    println!("statements: {:?}", module);

    let mut context = p7::interpreter::context::Context::new();
    context.load_module(module);
    context.push_function("test", Vec::new());
    context.resume().unwrap();

    println!("stacklen: {}", context.stack[0].stack.len());
    println!("stack 0: {:?}", context.stack[0].stack[0]);

    Ok(())
}


fn main() {
    test_parser_with_file().unwrap()
}
