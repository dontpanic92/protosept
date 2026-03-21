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

use crate::errors::{SemanticError, SourcePos};

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
    pub(super) _current_module_path: String,
    // Track which variables have been moved (by their index in local scope)
    pub(super) moved_variables: std::collections::HashSet<u32>,
    // Stack of loop contexts for nested loops
    pub(super) loop_context_stack: Vec<LoopContext>,
    // String constant pool for string literals
    pub(super) string_constants: Vec<String>,
    // Track the containing type when generating methods (for Self resolution)
    pub(super) current_self_type: Option<Type>,
    pub(super) is_compiling_builtin: bool,
    // Track type parameters of the enclosing generic type (struct/enum) when processing methods
    pub(super) enclosing_type_params: Vec<String>,
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
            _current_module_path: "$root".to_string(),
            moved_variables: std::collections::HashSet::new(),
            loop_context_stack: Vec::new(),
            string_constants: Vec::new(),
            current_self_type: None,
            is_compiling_builtin: false,
            enclosing_type_params: Vec::new(),
        }
    }

    pub fn generate(&mut self, statements: Vec<Statement>) -> SaResult<Module> {
        // Eagerly load the builtin module so that builtin intrinsic functions
        // (e.g. __script_dir__) are in scope for all user code.
        self.load_builtin();

        // Two-pass compilation for forward reference support:
        // Pass 1: Process all type/function declarations (register signatures)
        // Pass 2: Generate all code (bodies can reference any declaration)

        // Separate declarations from executable statements
        let mut declarations = Vec::new();
        let mut other_statements = Vec::new();
        for statement in statements {
            match &statement {
                Statement::FunctionDeclaration(_)
                | Statement::StructDeclaration { .. }
                | Statement::EnumDeclaration { .. }
                | Statement::ProtoDeclaration { .. }
                | Statement::Import { .. } => {
                    declarations.push(statement);
                }
                _ => {
                    other_statements.push(statement);
                }
            }
        }

        // Pass 1: Register all declarations (signatures only, no bodies)
        // Process imports first so types from other modules are available
        let mut function_decls = Vec::new();
        for statement in declarations {
            match statement {
                Statement::Import { module_path, alias } => {
                    self.generate_import(module_path, alias)?;
                }
                Statement::StructDeclaration {
                    is_pub,
                    name,
                    attributes,
                    conformance,
                    type_parameters,
                    fields,
                    methods,
                } => {
                    self.generate_struct_decl(
                        is_pub, name, attributes, conformance, type_parameters, fields, methods,
                    )?;
                }
                Statement::EnumDeclaration {
                    is_pub,
                    name,
                    attributes,
                    conformance,
                    type_parameters,
                    values,
                    methods,
                } => {
                    self.generate_enum_decl(
                        is_pub, name, attributes, conformance, type_parameters, values, methods,
                    )?;
                }
                Statement::ProtoDeclaration {
                    is_pub,
                    name,
                    attributes,
                    methods,
                } => {
                    self.generate_proto_decl(is_pub, name, attributes, methods)?;
                }
                Statement::FunctionDeclaration(decl) => {
                    // Register signature only (no body generation)
                    self.register_function_signature(&decl)?;
                    function_decls.push(decl);
                }
                _ => unreachable!(),
            }
        }

        // Pass 2: Generate function bodies
        for decl in function_decls {
            self.generate_function_body(decl)?;
        }

        // Pass 3: Generate executable statements (let bindings, expressions at top level)
        for statement in other_statements {
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
                    // Import top-level public functions from the builtin module
                    // into the main symbol table so they can be called without
                    // a module qualifier (e.g. `__script_dir__()`).
                    self.import_builtin_functions(&module);

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

    /// Import top-level public intrinsic functions from the builtin module into
    /// the main generator's symbol table so they are callable without a module
    /// qualifier (e.g. `__script_dir__()`).
    fn import_builtin_functions(&mut self, module: &Module) {
        let Some(root) = module.symbols.get(0) else {
            return;
        };

        for (_name, child_id) in &root.children {
            let Some(sym) = module.symbols.get(*child_id as usize) else {
                continue;
            };
            let SymbolKind::Function { func_id, address } = &sym.kind else {
                continue;
            };
            let Some(func) = module.functions.get(*func_id as usize) else {
                continue;
            };
            // Only import intrinsic functions (those are the ones we can actually
            // execute at runtime via InvokeHost).
            if func.intrinsic_name.is_none() {
                continue;
            }
            // Register the function in the main symbol table
            let new_func_id = self.symbol_table.add_function(func.clone());
            let new_symbol = crate::semantic::Symbol::new(
                sym.name.clone(),
                sym.qualified_name.clone(),
                SymbolKind::Function {
                    func_id: new_func_id,
                    address: *address,
                },
            );
            self.symbol_table.insert_symbol(new_symbol);
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

    /// Create a new generator for compiling imported modules
    fn new_for_module(
        module_provider: Box<dyn crate::ModuleProvider>,
        module_path: String,
    ) -> Self {
        // When compiling the builtin module itself, skip preloading builtins
        let is_builtin = module_path == "builtin";
        let mut generator = Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
            compiling_modules: std::collections::HashSet::new(),
            _current_module_path: module_path.clone(),
            moved_variables: std::collections::HashSet::new(),
            loop_context_stack: Vec::new(),
            string_constants: Vec::new(),
            current_self_type: None,
            is_compiling_builtin: is_builtin,
            enclosing_type_params: Vec::new(),
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

    /// Pass 1 helper: Register a function's name and signature in the symbol table
    /// without generating its body. This enables forward references.
    fn register_function_signature(&mut self, declaration: &FunctionDeclaration) -> SaResult<()> {
        let _ = declaration.is_pub;

        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(declaration.name.name.clone());

        let own_type_param_names: Vec<String> = declaration
            .type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let mut all_type_param_names = self.enclosing_type_params.clone();
        all_type_param_names.extend(own_type_param_names.clone());

        let is_generic = !all_type_param_names.is_empty();

        let (params, generic_param_types) = if is_generic {
            let params: Vec<Variable> = declaration
                .parameters
                .iter()
                .map(|arg| {
                    Ok(Variable {
                        name: arg.name.name.clone(),
                        ty: Type::Primitive(PrimitiveType::Unit),
                        is_mutable: false,
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
            let params = declaration
                .parameters
                .iter()
                .map(|arg| {
                    self.get_semantic_type(&arg.arg_type).and_then(|ty| {
                        Ok(Variable {
                            name: arg.name.name.clone(),
                            ty,
                            is_mutable: false,
                        })
                    })
                })
                .collect::<SaResult<Vec<Variable>>>()?;
            (params, None)
        };

        let param_defaults: Vec<Option<Expression>> = declaration
            .parameters
            .iter()
            .map(|param| param.default_value.clone())
            .collect();

        let (return_type, generic_return_type) = if is_generic {
            let generic_ret = declaration.return_type.clone();
            (Type::Primitive(PrimitiveType::Unit), generic_ret)
        } else {
            let ret_type = if let Some(ret) = &declaration.return_type {
                self.get_semantic_type(ret)?
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
            type_parameters: all_type_param_names.clone(),
            generic_param_types,
            generic_return_type,
            generic_body: if is_generic {
                Some(declaration.body.clone())
            } else {
                None
            },
            monomorphization: None,
        };

        let func_id = self.symbol_table.add_function(ty);

        // Use a placeholder address; will be patched in generate_function_body
        let symbol = Symbol::new(
            declaration.name.name.clone(),
            qualified_name,
            SymbolKind::Function {
                func_id,
                address: 0, // placeholder — patched in pass 2
            },
        );

        self.symbol_table.push_symbol(symbol);
        self.symbol_table.pop_symbol();

        Ok(())
    }

    /// Pass 2 helper: Generate the body of a previously registered function.
    fn generate_function_body(&mut self, declaration: FunctionDeclaration) -> SaResult<()> {
        let own_type_param_names: Vec<String> = declaration
            .type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();
        let mut all_type_param_names = self.enclosing_type_params.clone();
        all_type_param_names.extend(own_type_param_names);
        let is_generic = !all_type_param_names.is_empty();

        // Generic functions defer body generation to monomorphization
        if is_generic {
            return Ok(());
        }

        // Find the symbol we registered in pass 1
        let symbol_id = self
            .symbol_table
            .find_symbol(&declaration.name.name)
            .ok_or_else(|| {
                SemanticError::FunctionNotFound {
                    name: declaration.name.name.clone(),
                    pos: Some(SourcePos {
                        line: declaration.name.line,
                        col: declaration.name.col,
                        module: Some(self._current_module_path.clone()),
                    }),
                }
            })?;

        // Patch the address to where we are now in the bytecode
        let address = self.builder.next_address() as u32;
        if let Some(sym) = self.symbol_table.symbols.get_mut(symbol_id as usize) {
            if let SymbolKind::Function {
                address: ref mut func_addr,
                ..
            } = sym.kind
            {
                *func_addr = address;
            }
        }

        // Resolve params for body generation
        let params = declaration
            .parameters
            .iter()
            .map(|arg| {
                self.get_semantic_type(&arg.arg_type).and_then(|ty| {
                    Ok(Variable {
                        name: arg.name.name.clone(),
                        ty,
                        is_mutable: false,
                    })
                })
            })
            .collect::<SaResult<Vec<Variable>>>()?;

        let return_type = if let Some(ret) = &declaration.return_type {
            self.get_semantic_type(ret)?
        } else {
            Type::Primitive(PrimitiveType::Unit)
        };

        let intrinsic_name = Self::extract_intrinsic_name(&declaration.attributes);

        // Push the symbol onto the chain for body generation
        self.symbol_table.push_existing_symbol(symbol_id);

        self.clear_moved_variables();
        self.local_scope = Some(LocalSymbolScope::new(params.clone()));

        let prev_self_type = self.current_self_type.clone();
        if self.symbol_table.symbol_chain.len() >= 2 {
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

        let _ty = if let Some(ref intrinsic_fn_name) = intrinsic_name {
            let host_fn_name_idx = self.add_string_constant(intrinsic_fn_name.clone());
            self.builder.call_host_function(host_fn_name_idx);
            return_type.clone()
        } else {
            self.generate_block(declaration.body, vec![])?
        };

        self.current_self_type = prev_self_type;
        self.builder.ret();
        self.local_scope = None;
        self.symbol_table.pop_symbol();

        Ok(())
    }

    pub(super) fn process_function_declaration(
        &mut self,
        declaration: FunctionDeclaration,
    ) -> SaResult<()> {
        // TODO: Store declaration.is_pub for module visibility checking
        let _ = declaration.is_pub;

        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(declaration.name.name.clone());

        // Extract type parameter names from the function itself
        let own_type_param_names: Vec<String> = declaration
            .type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        // Combine with enclosing type params (from generic struct/enum)
        // Enclosing params come first as they are "outer" type parameters
        let mut all_type_param_names = self.enclosing_type_params.clone();
        all_type_param_names.extend(own_type_param_names.clone());

        // A function needs deferred type resolution if it has its own type params
        // or if it's inside a generic struct/enum (has enclosing type params)
        let is_generic = !all_type_param_names.is_empty();

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
            type_parameters: all_type_param_names.clone(),
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
