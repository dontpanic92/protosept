pub mod ast;
pub mod bytecode;
pub mod embedding;
pub mod errors;
pub mod intern;
pub mod interpreter;
pub mod lexer;
pub mod native_abi;
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

    /// Load a module on behalf of another canonical module.
    ///
    /// Package-aware providers can use `requester` to enforce dependency
    /// visibility. Legacy providers inherit the unrestricted behavior.
    fn load_module_from(&self, _requester: &str, module_path: &str) -> Option<String> {
        self.load_module(module_path)
    }

    /// Whether `module_path` represents a directory index such as `mod.p7`.
    ///
    /// Relative imports from directory modules resolve beneath the module
    /// itself, while relative imports from leaf modules resolve beside it.
    fn module_is_directory(&self, _module_path: &str) -> bool {
        false
    }

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
    directory_modules: Rc<std::collections::HashSet<String>>,
}

impl Default for InMemoryModuleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryModuleProvider {
    pub fn new() -> Self {
        InMemoryModuleProvider {
            modules: Rc::new(HashMap::new()),
            directory_modules: Rc::new(std::collections::HashSet::new()),
        }
    }

    pub fn add_module(&mut self, module_path: String, source: String) {
        // We need to make the Rc mutable, so we convert to owned HashMap
        let mut map = (*self.modules).clone();
        map.insert(module_path, source);
        self.modules = Rc::new(map);
    }

    pub fn add_directory_module(&mut self, module_path: String, source: String) {
        self.add_module(module_path.clone(), source);
        let mut modules = (*self.directory_modules).clone();
        modules.insert(module_path);
        self.directory_modules = Rc::new(modules);
    }
}

impl ModuleProvider for InMemoryModuleProvider {
    fn load_module(&self, module_path: &str) -> Option<String> {
        self.modules.get(module_path).cloned()
    }

    fn clone_boxed(&self) -> Box<dyn ModuleProvider> {
        Box::new(self.clone())
    }

    fn module_is_directory(&self, module_path: &str) -> bool {
        self.directory_modules.contains(module_path)
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
            "builtin" => Some(include_str!("../builtin.p7").to_string()),
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

    fn load_module_from(&self, requester: &str, module_path: &str) -> Option<String> {
        Self::get_builtin_module(module_path)
            .or_else(|| self.inner.load_module_from(requester, module_path))
    }

    fn module_is_directory(&self, module_path: &str) -> bool {
        self.inner.module_is_directory(module_path)
    }
}

pub fn compile(contents: String) -> Result<bytecode::Module, Proto7Error> {
    compile_with_provider(contents, Box::new(NoModuleProvider))
}

pub fn compile_with_provider(
    contents: String,
    provider: Box<dyn ModuleProvider>,
) -> Result<bytecode::Module, Proto7Error> {
    compile_module_with_provider(contents, "$root", provider)
}

/// Compile a root module with an explicit canonical module path.
///
/// Package-aware hosts should use a path such as `my_package.main`. Legacy
/// script hosts may continue using [`compile_with_provider`], whose root path
/// remains `$root`.
pub fn compile_module_with_provider(
    contents: String,
    module_path: &str,
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

    let mut codegen =
        bytecode::codegen::Generator::new_with_module_path(provider_with_builtins, module_path);
    let module = codegen.generate(statements)?;

    Ok(module)
}

pub fn run(
    module: bytecode::Module,
    entrypoint: &str,
) -> Result<interpreter::context::Data, Proto7Error> {
    run_with_options(module, entrypoint, RunOptions::default())
}

/// Options for configuring the runtime execution environment.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// The containing directory of the entry script, if it originates from a
    /// filesystem path.  When set, the built-in `__script_dir__` identifier
    /// evaluates to `Some(dir)` at runtime; otherwise it is `null`.
    pub script_dir: Option<String>,
}

pub fn run_with_options(
    module: bytecode::Module,
    entrypoint: &str,
    options: RunOptions,
) -> Result<interpreter::context::Data, Proto7Error> {
    let mut context = interpreter::context::Context::new();
    context.set_script_dir(options.script_dir);
    context.load_module(module);
    context.push_function(entrypoint, Vec::new());
    context.resume().map_err(Proto7Error::RuntimeError)?;

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

pub fn compile_and_run_with_provider(
    contents: String,
    entrypoint: &str,
    provider: Box<dyn ModuleProvider>,
) -> Result<interpreter::context::Data, Proto7Error> {
    let module = compile_with_provider(contents, provider)?;
    run(module, entrypoint)
}

pub fn compile_and_run_with_provider_and_options(
    contents: String,
    entrypoint: &str,
    provider: Box<dyn ModuleProvider>,
    options: RunOptions,
) -> Result<interpreter::context::Data, Proto7Error> {
    let module = compile_with_provider(contents, provider)?;
    run_with_options(module, entrypoint, options)
}
