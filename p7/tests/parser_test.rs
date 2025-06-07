use std::error::Error;
use std::fs;
use std::path::Path;

use binrw::BinRead;
use p7::bytecode::Instruction;

#[test]
fn test_parser_with_file() -> Result<(), Box<dyn Error>> {
    let file_path = Path::new("../tests/test2.p7");
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

    println!("module: {:?}", module);

    let mut insts = vec![];
    let mut cursor = std::io::Cursor::new(&module.instructions);

    while cursor.position() < module.instructions.len() as u64 {
        let inst = Instruction::read(&mut cursor)?;
        insts.push(inst);
    }

    println!("insts: {:?}", insts);

    Ok(())
}
