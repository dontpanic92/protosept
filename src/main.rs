use std::{error::Error, fs, path::Path};

pub mod lexer;
pub mod parser;

fn test_parser_with_file() -> Result<(), Box<dyn Error>> {
    let file_path = Path::new("tests/test2.p7");
    let contents = fs::read_to_string(file_path)?;

    let mut lexer = p7lang::lexer::Lexer::new(contents);
    let mut tokens = vec![];
    
    loop {
        let token = lexer.next_token();
        if token.token_type == p7lang::lexer::TokenType::EOF {
            break;
        } else {
            tokens.push(token);
        }
    }

    let mut parser = p7lang::parser::Parser::new(tokens);
    let statements = parser.parse()?;

    let mut codegen = p7lang::bytecode::codegen::Generator::new();
    let bytecode = codegen.generate(statements)?;

    println!("statements: {:?}", bytecode);

    Ok(())
}


fn main() {
    test_parser_with_file().unwrap()
}
