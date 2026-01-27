use std::io::Cursor;

use binrw::BinRead;
use p7::bytecode::{Instruction, Module};
use p7::semantic::{SymbolKind, Type, UserDefinedType};

#[derive(Debug)]
struct InstEntry {
    offset: u32,
    size: u32,
    inst: Instruction,
}

#[derive(Debug)]
struct DecodeResult {
    entries: Vec<InstEntry>,
    error: Option<String>,
}

#[derive(Debug)]
struct FunctionMeta {
    symbol_id: u32,
    qualified_name: String,
    address: u32,
    type_id: u32,
    param_names: Vec<String>,
    param_types: Vec<Type>,
    return_type: Option<Type>,
}

pub fn disassemble_module(module: &Module) -> String {
    let bytes_len = module.instructions.len();
    let decode = decode_instructions(&module.instructions);

    let mut output = String::new();
    output.push_str("p7 disassembly\n");
    output.push_str(&format!("bytes: {}\n", bytes_len));
    output.push_str(&format!("instructions: {}\n", decode.entries.len()));
    output.push_str(&format!("symbols: {}\n", module.symbols.len()));
    output.push_str(&format!("types: {}\n\n", module.types.len()));

    let mut functions = collect_functions(module);
    functions.sort_by_key(|f| f.address);

    let mut cursor = 0u32;
    let bytecode_end = bytes_len as u32;

    for (index, func) in functions.iter().enumerate() {
        let start = func.address;
        let end = functions
            .get(index + 1)
            .map(|f| f.address)
            .unwrap_or(bytecode_end);

        if cursor < start {
            output.push_str(&format!("data @{}\n", hex_offset(cursor)));
            output.push_str("  ---\n");
            for entry in decode
                .entries
                .iter()
                .filter(|e| e.offset >= cursor && e.offset < start)
            {
                output.push_str("  ");
                output.push_str(&format_instruction(entry, module));
                output.push('\n');
            }
            output.push('\n');
        }

        output.push_str(&format!(
            "func {} @{}\n",
            func.qualified_name,
            hex_offset(start)
        ));
        output.push_str(&format!("  params: {}\n", format_params(func)));
        output.push_str(&format!(
            "  returns: {}\n",
            func.return_type
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ));
        output.push_str(&format!("  symbol_id: {}\n", func.symbol_id));
        output.push_str(&format!("  type_id: {}\n", func.type_id));
        output.push_str(&format!("  size: {}\n", end.saturating_sub(start)));
        output.push_str("  ---\n");

        for entry in decode
            .entries
            .iter()
            .filter(|e| e.offset >= start && e.offset < end)
        {
            output.push_str("  ");
            output.push_str(&format_instruction(entry, module));
            output.push('\n');
        }
        output.push('\n');

        cursor = end;
    }

    if cursor < bytecode_end {
        output.push_str(&format!("data @{}\n", hex_offset(cursor)));
        output.push_str("  ---\n");
        for entry in decode
            .entries
            .iter()
            .filter(|e| e.offset >= cursor && e.offset < bytecode_end)
        {
            output.push_str("  ");
            output.push_str(&format_instruction(entry, module));
            output.push('\n');
        }
        output.push('\n');
    }

    if let Some(error) = decode.error {
        output.push_str(&format!("error: {}\n", error));
    }

    output
}

fn decode_instructions(bytes: &[u8]) -> DecodeResult {
    let mut cursor = Cursor::new(bytes);
    let mut entries = Vec::new();
    let mut error = None;

    while cursor.position() < bytes.len() as u64 {
        let start = cursor.position() as u32;
        match Instruction::read(&mut cursor) {
            Ok(inst) => {
                let end = cursor.position() as u32;
                entries.push(InstEntry {
                    offset: start,
                    size: end.saturating_sub(start),
                    inst,
                });
            }
            Err(e) => {
                error = Some(format!("failed to decode at {}: {}", hex_offset(start), e));
                break;
            }
        }
    }

    DecodeResult { entries, error }
}

fn collect_functions(module: &Module) -> Vec<FunctionMeta> {
    let mut functions = Vec::new();

    for (symbol_id, symbol) in module.symbols.iter().enumerate() {
        if let SymbolKind::Function { type_id, address } = symbol.kind {
            let (param_names, param_types, return_type) = match module.types.get(type_id as usize) {
                Some(UserDefinedType::Function(func)) => (
                    func.param_names.clone(),
                    func.params.clone(),
                    Some(func.return_type.clone()),
                ),
                _ => (Vec::new(), Vec::new(), None),
            };

            functions.push(FunctionMeta {
                symbol_id: symbol_id as u32,
                qualified_name: symbol.qualified_name.clone(),
                address,
                type_id,
                param_names,
                param_types,
                return_type,
            });
        }
    }

    functions
}

fn format_params(func: &FunctionMeta) -> String {
    if func.param_names.is_empty() {
        return "()".to_string();
    }

    let mut parts = Vec::new();
    for (idx, name) in func.param_names.iter().enumerate() {
        let ty = func
            .param_types
            .get(idx)
            .map(|t| t.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        parts.push(format!("{}:{}", name, ty));
    }

    format!("({})", parts.join(", "))
}

fn format_instruction(entry: &InstEntry, module: &Module) -> String {
    let offset_hex = hex_offset(entry.offset);

    match &entry.inst {
        Instruction::Ldi(value) => format!("{}  ldi {}", offset_hex, value),
        Instruction::Ldf(value) => format!("{}  ldf {}", offset_hex, value),
        Instruction::Ldvar(id) => format!("{}  ldvar {}", offset_hex, id),
        Instruction::Stvar(id) => format!("{}  stvar {}", offset_hex, id),
        Instruction::Ldpar(id) => format!("{}  ldpar {}", offset_hex, id),
        Instruction::Add => format!("{}  add", offset_hex),
        Instruction::Sub => format!("{}  sub", offset_hex),
        Instruction::Mul => format!("{}  mul", offset_hex),
        Instruction::Div => format!("{}  div", offset_hex),
        Instruction::Mod => format!("{}  mod", offset_hex),
        Instruction::Neg => format!("{}  neg", offset_hex),
        Instruction::And => format!("{}  and", offset_hex),
        Instruction::Or => format!("{}  or", offset_hex),
        Instruction::Not => format!("{}  not", offset_hex),
        Instruction::Eq => format!("{}  eq", offset_hex),
        Instruction::Neq => format!("{}  neq", offset_hex),
        Instruction::Lt => format!("{}  lt", offset_hex),
        Instruction::Gt => format!("{}  gt", offset_hex),
        Instruction::Lte => format!("{}  lte", offset_hex),
        Instruction::Gte => format!("{}  gte", offset_hex),
        Instruction::Jmp(address) => {
            format!("{}  jmp {}", offset_hex, hex_offset(*address))
        }
        Instruction::Jif(address) => {
            format!("{}  jif {}", offset_hex, hex_offset(*address))
        }
        Instruction::Call(symbol_id) => {
            let annotation = module
                .symbols
                .get(*symbol_id as usize)
                .map(|symbol| format!("; name={}", symbol.qualified_name));
            if let Some(annotation) = annotation {
                format!("{}  call {}  {}", offset_hex, symbol_id, annotation)
            } else {
                format!("{}  call {}", offset_hex, symbol_id)
            }
        }
        Instruction::Ret => format!("{}  ret", offset_hex),
        Instruction::Pop => format!("{}  pop", offset_hex),
        Instruction::Dup => format!("{}  dup", offset_hex),
        Instruction::Throw => format!("{}  throw", offset_hex),
        Instruction::CheckException(address) => {
            format!("{}  check_exception {}", offset_hex, hex_offset(*address))
        }
        Instruction::UnwrapException => format!("{}  unwrap_exception", offset_hex),
        Instruction::Ldfield(index) => format!("{}  ldfield {}", offset_hex, index),
        Instruction::Stfield(index) => format!("{}  stfield {}", offset_hex, index),
        Instruction::NewStruct(count) => format!("{}  newstruct {}", offset_hex, count),
        Instruction::BoxAlloc => format!("{}  box_alloc", offset_hex),
        Instruction::BoxDeref => format!("{}  box_deref", offset_hex),
        Instruction::BoxToProto(struct_id, proto_id) => format!("{}  box_to_proto {} {}", offset_hex, struct_id, proto_id),
        Instruction::CallProtoMethod(proto_id, method_hash) => format!("{}  call_proto_method {} {:#x}", offset_hex, proto_id, method_hash),
    }
}

fn hex_offset(offset: u32) -> String {
    format!("{:06x}", offset)
}

#[cfg(test)]
mod tests {
    use super::disassemble_module;
    use p7::bytecode::Module;
    use p7::bytecode::builder::ByteCodeBuilder;

    #[test]
    fn disassembles_basic_instructions() {
        let mut builder = ByteCodeBuilder::new();
        builder.ldi(42);
        builder.ret();

        let module = Module {
            instructions: builder.get_bytecode(),
            symbols: Vec::new(),
            types: Vec::new(),
        };

        let output = disassemble_module(&module);

        assert!(output.contains("ldi 42"));
        assert!(output.contains("ret"));
    }
}
