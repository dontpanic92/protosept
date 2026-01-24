use std::error::Error;
use std::fs;
use std::path::PathBuf;

use binrw::BinRead;
use p7::bytecode::Instruction;

fn parse_and_generate(filename: &str) -> Result<Vec<Instruction>, Box<dyn Error>> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push("..");
    file_path.push("tests");
    file_path.push(filename);
    
    let contents = fs::read_to_string(&file_path)?;

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

    let mut insts = vec![];
    let mut cursor = std::io::Cursor::new(&module.instructions);

    while cursor.position() < module.instructions.len() as u64 {
        let inst = Instruction::read(&mut cursor)?;
        insts.push(inst);
    }

    Ok(insts)
}

#[test]
fn test_if_expression_codegen() -> Result<(), Box<dyn Error>> {
    let insts = parse_and_generate("test_if_expression.p7")?;
    
    // Verify that we have jump instructions (Jif and/or Jmp) for the if-else
    let has_jif = insts.iter().any(|inst| matches!(inst, Instruction::Jif(_)));
    let has_jmp = insts.iter().any(|inst| matches!(inst, Instruction::Jmp(_)));
    
    assert!(has_jif, "Expected Jif (jump if false) instruction for if expression");
    
    // If there's an else clause, we should also have a Jmp to skip over it
    assert!(has_jmp, "Expected Jmp (unconditional jump) instruction to skip else clause");

    Ok(())
}

#[test]
fn test_if_without_else_codegen() -> Result<(), Box<dyn Error>> {
    let insts = parse_and_generate("test_if_no_else.p7")?;
    
    // Should have at least one Jif for the if condition
    let has_jif = insts.iter().any(|inst| matches!(inst, Instruction::Jif(_)));
    assert!(has_jif, "Expected Jif (jump if false) instruction for if expression");

    Ok(())
}

#[test]
fn test_nested_if_codegen() -> Result<(), Box<dyn Error>> {
    let insts = parse_and_generate("test_nested_if.p7")?;
    
    // Count the number of jump instructions - should have multiple for nested ifs
    let jif_count = insts.iter().filter(|inst| matches!(inst, Instruction::Jif(_))).count();
    let jmp_count = insts.iter().filter(|inst| matches!(inst, Instruction::Jmp(_))).count();
    
    assert!(jif_count >= 2, "Expected at least 2 Jif instructions for nested if expressions, got {}", jif_count);
    assert!(jmp_count >= 2, "Expected at least 2 Jmp instructions for nested if expressions, got {}", jmp_count);

    Ok(())
}

#[test]
fn test_if_with_bool_logic_codegen() -> Result<(), Box<dyn Error>> {
    let insts = parse_and_generate("test_if_complex.p7")?;
    
    // Should have jump instructions for the if
    let has_jif = insts.iter().any(|inst| matches!(inst, Instruction::Jif(_)));
    assert!(has_jif, "Expected Jif instruction for if with boolean condition");
    
    // Should have comparison instruction (Gt for >)
    let has_gt = insts.iter().any(|inst| matches!(inst, Instruction::Gt));
    assert!(has_gt, "Expected Gt instruction for > operator");

    Ok(())
}
