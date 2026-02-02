use core::panic;

use crate::{
    ast::{Expression, FunctionDeclaration, Statement},
    bytecode::builder::ByteCodeBuilder,
    semantic::{
        Function, FunctionId, LocalSymbolScope, PrimitiveType, Symbol, SymbolKind, SymbolTable,
        Type, TypeDefinition, Variable,
    },
};

use super::Module;

mod args;
mod control_flow;
pub(crate) use control_flow::LoopContext;
mod call;
mod enums;
mod expression;
mod helpers;
mod monomorph;
mod patterns;
mod statement;
mod structs;
mod type_check;

use crate::errors::SemanticError;

pub type SaResult<T> = Result<T, SemanticError>;

// Synthetic position values for compiler-generated code (e.g., monomorphization)
pub(super) const SYNTHETIC_LINE: usize = 0;
pub(super) const SYNTHETIC_COL: usize = 0;

pub struct ExternSymbolId {
    pub module_path: String,
    pub symbol_id: u32,
}

pub struct Generator {
    pub(super) builder: ByteCodeBuilder,
    pub(super) symbol_table: SymbolTable,
    pub(super) local_scope: Option<LocalSymbolScope>,
    pub(super) pending_monomorphizations:
        Vec<(u32, FunctionId, Vec<Statement>, Vec<String>, Vec<Type>)>, // (symbol_id, func_id, body, param_names, params)
    pub(super) module_provider: Box<dyn crate::ModuleProvider>,
    pub(super) imported_modules: std::collections::HashMap<String, Module>,
    pub(super) compiling_modules: std::collections::HashSet<String>,
    pub(super) current_module_path: String,
    // Track which variables have been moved (by their index in local scope)
    pub(super) moved_variables: std::collections::HashSet<u32>,
    // Stack of loop contexts for nested loops
    pub(super) loop_context_stack: Vec<LoopContext>,
    // String constant pool for string literals
    pub(super) string_constants: Vec<String>,
    // Track the containing type when generating methods (for Self resolution)
    pub(super) current_self_type: Option<Type>,
    pub(super) is_compiling_builtin: bool,
}

impl Generator {
    /// Resolve a symbol exported from an imported module by name
    fn resolve_module_member<'a>(
        &'a self,
        module_path: &str,
        member: &str,
    ) -> Option<&'a crate::semantic::Symbol> {
        let module = self.imported_modules.get(module_path)?;
        let root = module.symbols.get(0)?;
        let child_id = root.children.get(member)?;
        module.symbols.get(*child_id as usize)
    }
    pub fn new(module_provider: Box<dyn crate::ModuleProvider>) -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
            compiling_modules: std::collections::HashSet::new(),
            current_module_path: "$root".to_string(),
            moved_variables: std::collections::HashSet::new(),
            loop_context_stack: Vec::new(),
            string_constants: Vec::new(),
            current_self_type: None,
            is_compiling_builtin: false,
        }
    }

    pub fn generate(&mut self, statements: Vec<Statement>) -> SaResult<Module> {
        for statement in statements {
            self.generate_statement(statement)?;
        }
        // Generate bytecode for all pending monomorphizations
        // These were deferred to avoid inline generation during function bodies
        while !self.pending_monomorphizations.is_empty() {
            // Take all pending monomorphizations
            let pending = std::mem::take(&mut self.pending_monomorphizations);

            for (symbol_id, _type_id, body, param_names, params) in pending {
                // Generate the monomorphized function's bytecode
                let address = self.builder.next_address() as u32;

                // Update the symbol's address
                if let Some(sym) = self.symbol_table.symbols.get_mut(symbol_id as usize) {
                    if let SymbolKind::Function {
                        address: ref mut func_addr,
                        ..
                    } = sym.kind
                    {
                        *func_addr = address;
                    }
                }

                // Set up local scope and generate body
                let variables: Vec<Variable> = param_names
                    .iter()
                    .zip(params.iter())
                    .map(|(name, ty)| Variable {
                        name: name.clone(),
                        ty: ty.clone(),
                        is_mutable: false, // Parameters are immutable
                    })
                    .collect();

                let saved_local_scope = self.local_scope.take();
                self.clear_moved_variables(); // Clear moved variables when entering new function
                self.local_scope = Some(LocalSymbolScope::new(variables));

                let _ = self.generate_block(body, vec![])?;
                self.builder.ret();

                self.local_scope = saved_local_scope;
            }
        }

        Ok(Module {
            instructions: self.builder.get_bytecode(),
            symbols: self.symbol_table.symbols.clone(),
            functions: self.symbol_table.functions.clone(),
            types: self.symbol_table.types.clone(),
            string_constants: self.string_constants.clone(),
            imported_modules: self
                .imported_modules
                .iter()
                .map(|(k, v)| (k.clone(), Box::new(v.clone())))
                .collect(),
        })
    }

    /// Try to load a builtin type on-demand
    pub(super) fn load_builtin(&mut self) {
        // Avoid recursive loading
        if self.is_compiling_builtin {
            return;
        }

        const MODULE_PATH: &str = "builtin";

        // Check if already loaded
        if self.imported_modules.contains_key(MODULE_PATH) {
            return;
        }

        // Load the builtin module
        if let Some(source) = self.module_provider.load_module(MODULE_PATH) {
            match self.compile_module(MODULE_PATH, source) {
                Ok(module) => {
                    // Store the compiled module
                    self.imported_modules
                        .insert(MODULE_PATH.to_string(), module);
                }
                Err(e) => {
                    eprintln!("DEBUG try_load_builtin_type: compile error = {:?}", e);
                }
            }
        } else {
            panic!(
                "Builtin module '{}' not found in module provider",
                MODULE_PATH
            );
        }
    }

    /// Helper method to compile an imported module
    pub(super) fn compile_module(&mut self, module_path: &str, source: String) -> SaResult<Module> {
        // Cycle detection
        if self.compiling_modules.contains(module_path) {
            return Err(SemanticError::Other(format!(
                "Cyclic import detected: {}",
                module_path
            )));
        }
        self.compiling_modules.insert(module_path.to_string());

        // Parse the module source
        let mut lexer = crate::lexer::Lexer::new(source);
        let mut tokens = vec![];

        loop {
            let token = lexer.next_token();
            if token.token_type == crate::lexer::TokenType::EOF {
                break;
            } else {
                tokens.push(token);
            }
        }

        let mut parser = crate::parser::Parser::new(tokens);
        let statements = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("DEBUG compile_module: parse error = {:?}", e);
                self.compiling_modules.remove(module_path);
                return Err(SemanticError::Other(format!(
                    "Parse error in imported module: {:?}",
                    e
                )));
            }
        };

        // Create a new generator for the imported module with a cloned provider
        let mut module_gen =
            Generator::new_for_module(self.module_provider.clone_boxed(), module_path.to_string());
        let result = module_gen.generate(statements);
        self.compiling_modules.remove(module_path);
        result
    }

    /// Create a new generator for compiling imported modules (skips builtin preload to avoid recursion)
    fn new_for_module(
        module_provider: Box<dyn crate::ModuleProvider>,
        module_path: String,
    ) -> Self {
        let mut generator = Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
            compiling_modules: std::collections::HashSet::new(),
            current_module_path: module_path.clone(),
            moved_variables: std::collections::HashSet::new(),
            loop_context_stack: Vec::new(),
            string_constants: Vec::new(),
            current_self_type: None,
            is_compiling_builtin: true, // Skip builtin preload to avoid infinite recursion
        };
        // Override root module metadata with this module_path
        if let Some(root) = generator.symbol_table.symbols.get_mut(0) {
            root.name = module_path.clone();
            root.qualified_name = module_path.clone();
            root.kind = crate::semantic::SymbolKind::Module(0);
        }
        if let Some(root_info) = generator.symbol_table.modules.get_mut(0) {
            root_info.path = module_path.clone();
        }
        generator
    }

    pub(super) fn generate_block(
        &mut self,
        statements: Vec<Statement>,
        variables: Vec<Variable>,
    ) -> SaResult<Type> {
        self.local_scope.as_mut().unwrap().push_scope();
        for var in variables {
            self.local_scope
                .as_mut()
                .unwrap()
                .add_variable(var.name, var.ty, var.is_mutable)
                .unwrap();
        }

        let mut ty = Type::Primitive(PrimitiveType::Unit);

        // Normal block handling
        for statement in statements {
            ty = self.generate_statement(statement)?;
        }

        self.local_scope.as_mut().unwrap().pop_scope();

        Ok(ty)
    }

    /// Generate pattern matching code for a list of match arms.
    /// The scrutinee value should already be on the stack.

    pub(super) fn process_function_declaration(
        &mut self,
        declaration: FunctionDeclaration,
    ) -> SaResult<()> {
        // TODO: Store declaration.is_pub for module visibility checking
        let _ = declaration.is_pub;

        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(declaration.name.name.clone());

        // Extract type parameter names
        let type_param_names: Vec<String> = declaration
            .type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let is_generic = !type_param_names.is_empty();

        // For generic functions, store parsed parameter types; for non-generic, resolve them
        let (params, generic_param_types) = if is_generic {
            // For generic functions, store placeholder types and keep parsed types
            let params: Vec<Variable> = declaration
                .parameters
                .iter()
                .map(|arg| {
                    Ok(Variable {
                        name: arg.name.name.clone(),
                        ty: Type::Primitive(PrimitiveType::Unit), // Placeholder
                        is_mutable: false,                        // Parameters are immutable
                    })
                })
                .collect::<SaResult<Vec<Variable>>>()?;

            let generic_types: Vec<crate::ast::Type> = declaration
                .parameters
                .iter()
                .map(|arg| arg.arg_type.clone())
                .collect();

            (params, Some(generic_types))
        } else {
            // For non-generic functions, resolve types normally
            let params = declaration
                .parameters
                .iter()
                .map(|arg| {
                    self.get_semantic_type(&arg.arg_type).and_then(|ty| {
                        Ok(Variable {
                            name: arg.name.name.clone(),
                            ty,
                            is_mutable: false, // Parameters are immutable
                        })
                    })
                })
                .collect::<SaResult<Vec<Variable>>>()?;
            (params, None)
        };

        // Collect default expressions (AST expressions) for each parameter
        let param_defaults: Vec<Option<Expression>> = declaration
            .parameters
            .iter()
            .map(|param| param.default_value.clone())
            .collect();

        let (return_type, generic_return_type) = if is_generic {
            // For generic functions, store placeholder and keep parsed type
            let generic_ret = declaration.return_type.clone();
            (Type::Primitive(PrimitiveType::Unit), generic_ret)
        } else {
            let ret_type = if let Some(ret) = declaration.return_type {
                self.get_semantic_type(&ret)?
            } else {
                Type::Primitive(PrimitiveType::Unit)
            };

            if matches!(ret_type, Type::Reference(_)) {
                return Err(SemanticError::Other(
                    "Functions cannot return non-escapable ref types".to_string(),
                ));
            }

            (ret_type, None)
        };

        let intrinsic_name = Self::extract_intrinsic_name(&declaration.attributes);

        let ty = Function {
            qualified_name: qualified_name.clone(),
            params: params.iter().map(|var| var.ty.clone()).collect(),
            param_names: params.iter().map(|var| var.name.clone()).collect(),
            param_defaults: param_defaults.clone(),
            return_type: return_type.clone(),
            attributes: declaration.attributes.clone(),
            intrinsic_name: intrinsic_name.clone(),
            type_parameters: type_param_names.clone(),
            generic_param_types,
            generic_return_type,
            generic_body: if is_generic {
                Some(declaration.body.clone())
            } else {
                None
            },
            monomorphization: None, // This is the generic definition, not a monomorphization
        };

        let func_id = self.symbol_table.add_function(ty);

        // For generic functions, use placeholder address (no bytecode generated)
        // For non-generic functions, capture the address where body generation will start
        let symbol = Symbol::new(
            declaration.name.name,
            qualified_name,
            SymbolKind::Function {
                func_id,
                address: self.builder.next_address() as u32,
            },
        );

        self.symbol_table.push_symbol(symbol);

        // Only generate function body for non-generic functions
        // Generic functions will be monomorphized when called
        if !is_generic {
            self.clear_moved_variables(); // Clear moved variables when entering new function
            self.local_scope = Some(LocalSymbolScope::new(params.clone()));

            // Set current_self_type if this is a method inside a type/struct/enum
            // The current symbol is the function itself, so we need to look at its parent
            let prev_self_type = self.current_self_type.clone();
            if self.symbol_table.symbol_chain.len() >= 2 {
                // Get the parent symbol (not the current function symbol)
                let parent_symbol_id =
                    self.symbol_table.symbol_chain[self.symbol_table.symbol_chain.len() - 2];
                if let Some(parent_symbol) =
                    self.symbol_table.symbols.get(parent_symbol_id as usize)
                {
                    self.current_self_type = match &parent_symbol.kind {
                        SymbolKind::Type(type_id) => {
                            match &self.symbol_table.types[*type_id as usize] {
                                TypeDefinition::Struct(_) => Some(Type::Struct(*type_id)),
                                TypeDefinition::Enum(_) => Some(Type::Enum(*type_id)),
                                TypeDefinition::Proto(_) => None,
                            }
                        }
                        _ => None,
                    };
                }
            }

            // For intrinsic functions, generate a call_host_function instead of the body
            let ty = if let Some(ref intrinsic_fn_name) = intrinsic_name {
                // Emit call_host_function with the intrinsic name
                let host_fn_name_idx = self.add_string_constant(intrinsic_fn_name.clone());
                self.builder.call_host_function(host_fn_name_idx);
                return_type.clone()
            } else {
                self.generate_block(declaration.body, vec![])?
            };

            // Restore previous self_type
            self.current_self_type = prev_self_type;

            // Always emit Ret so unit-returning functions don't fall-through into subsequent code.
            // The VM `Ret` handler is responsible for popping the frame even if there's no value.
            if ty != Type::Primitive(PrimitiveType::Unit) {
                self.builder.ret();
            } else {
                self.builder.ret();
            }

            self.local_scope = None;
        }

        self.symbol_table.pop_symbol();

        Ok(())
    }
}
