use std::error::Error;
use std::fs;
use std::path::Path;

#[test]
fn test_parser_with_file() -> Result<(), Box<dyn Error>> {
    let file_path = Path::new("tests/test.p7");
    let contents = fs::read_to_string(file_path)?;

    let mut lexer = p7lang::lexer::Lexer::new(contents);
    let mut tokens = vec![];
    
    loop {
        let token = lexer.next_token();
        if token == p7lang::lexer::Token::EOF {
            break;
        } else {
            tokens.push(token);
        }
    }

    let mut parser = p7lang::parser::Parser::new(tokens);
    let statements = parser.parse();

    println!("statements: {:?}", statements);

    Ok(())
}
