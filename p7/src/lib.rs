pub mod ast;
pub mod bytecode;
pub mod errors;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod semantic;

use crate::errors::Proto7Error;
use std::collections::HashMap;

/// Trait for providing module sources to the compiler
/// Allows hosts to provide in-memory modules without filesystem dependencies
pub trait ModuleProvider {
    /// Load a module by its path (e.g., "test.test" or "std.collections.list")
    /// Returns the source code for the module, or None if not found
    fn load_module(&self, module_path: &str) -> Option<String>;
}

/// Default implementation that doesn't provide any modules
pub struct NoModuleProvider;

impl ModuleProvider for NoModuleProvider {
    fn load_module(&self, _module_path: &str) -> Option<String> {
        None
    }
}

/// Simple in-memory module provider using a HashMap
pub struct InMemoryModuleProvider {
    modules: HashMap<String, String>,
}

impl InMemoryModuleProvider {
    pub fn new() -> Self {
        InMemoryModuleProvider {
            modules: HashMap::new(),
        }
    }

    pub fn add_module(&mut self, module_path: String, source: String) {
        self.modules.insert(module_path, source);
    }
}

impl ModuleProvider for InMemoryModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        self.modules.get(module_path).cloned()
    }
}

pub fn compile(contents: String) -> Result<bytecode::Module, Proto7Error> {
    compile_with_provider(contents, &NoModuleProvider)
}

pub fn compile_with_provider(
    contents: String,
    provider: &dyn ModuleProvider,
) -> Result<bytecode::Module, Proto7Error> {
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

    let mut codegen = bytecode::codegen::Generator::new(provider);
    let module = codegen.generate(statements)?;

    Ok(module)
}

pub fn run(
    module: bytecode::Module,
    entrypoint: &str,
) -> Result<interpreter::context::Data, Proto7Error> {
    let mut context = interpreter::context::Context::new();
    context.load_module(module);
    context.push_function(entrypoint, Vec::new());
    context.resume().unwrap();

    let result = context.stack[0].stack.pop();
    match result {
        Some(value) => Ok(value),
        None => Err(Proto7Error::RuntimeError(
            errors::RuntimeError::StackUnderflow,
        )),
    }
}

pub fn compile_and_run(
    contents: String,
    entrypoint: &str,
) -> Result<interpreter::context::Data, Proto7Error> {
    let module = compile(contents.clone())?;
    run(module, entrypoint)
}
