pub mod ast;
pub mod bytecode;
pub mod errors;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod semantic;

use crate::errors::Proto7Error;
use std::collections::HashMap;
use std::rc::Rc;

/// Trait for providing module sources to the compiler
/// Allows hosts to provide in-memory modules without filesystem dependencies
pub trait ModuleProvider {
    /// Load a module by its path (e.g., "test.test" or "std.collections.list")
    /// Returns the source code for the module, or None if not found
    fn load_module(&self, module_path: &str) -> Option<String>;
    
    /// Clone the provider into a Box for recursive compilation
    fn clone_boxed(&self) -> Box<dyn ModuleProvider>;
}

/// Default implementation that doesn't provide any modules
#[derive(Clone)]
pub struct NoModuleProvider;

impl ModuleProvider for NoModuleProvider {
    fn load_module(&self, _module_path: &str) -> Option<String> {
        None
    }
    
    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }
}

/// Simple in-memory module provider using a HashMap
#[derive(Clone)]
pub struct InMemoryModuleProvider {
    modules: Rc<HashMap<String, String>>,
}

impl InMemoryModuleProvider {
    pub fn new() -> Self {
        InMemoryModuleProvider {
            modules: Rc::new(HashMap::new()),
        }
    }

    pub fn add_module(&mut self, module_path: String, source: String) {
        // We need to make the Rc mutable, so we convert to owned HashMap
        let mut map = (*self.modules).clone();
        map.insert(module_path, source);
        self.modules = Rc::new(map);
    }
}

impl ModuleProvider for InMemoryModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        self.modules.get(module_path).cloned()
    }
    
    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }
}

/// Module provider that includes builtin modules and wraps another provider
#[derive(Clone)]
pub struct BuiltinModuleProvider {
    inner: Rc<Box<dyn ModuleProvider>>,
}

impl BuiltinModuleProvider {
    pub fn new(inner: Box<dyn ModuleProvider>) -> Self {
        BuiltinModuleProvider {
            inner: Rc::new(inner),
        }
    }
    
    fn get_builtin_module(module_path: &str) -> Option<String> {
        match module_path {
            "builtin.string" => Some(include_str!("../builtin/string.p7").to_string()),
            _ => None,
        }
    }
}

impl ModuleProvider for BuiltinModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        // First try to load from builtin modules
        if let Some(builtin) = Self::get_builtin_module(module_path) {
            return Some(builtin);
        }
        // Fall back to the inner provider
        self.inner.load_module(module_path)
    }
    
    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }
}

pub fn compile(contents: String) -> Result<bytecode::Module, Proto7Error> {
    compile_with_provider(contents, Box::new(NoModuleProvider))
}

pub fn compile_with_provider(
    contents: String,
    provider: Box<dyn ModuleProvider>,
) -> Result<bytecode::Module, Proto7Error> {
    // Wrap the provider with builtin support
    let provider_with_builtins = Box::new(BuiltinModuleProvider::new(provider));
    
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

    let mut codegen = bytecode::codegen::Generator::new(provider_with_builtins);
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
    context.resume().map_err(|e| Proto7Error::RuntimeError(e))?;

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
