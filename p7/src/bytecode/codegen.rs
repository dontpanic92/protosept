use core::panic;

use crate::errors::SourcePos;
use crate::{
    ast::{Expression, FunctionCall, FunctionDeclaration, Identifier, MatchArm, Statement},
    bytecode::{Instruction, builder::ByteCodeBuilder},
    semantic::{
        Enum, Function, LocalSymbolScope, PrimitiveType, Proto, Struct, Symbol, SymbolKind,
        SymbolTable, Type, TypeId, UserDefinedType, Variable,
    },
};

use super::Module;

use crate::errors::SemanticError;

pub type SaResult<T> = Result<T, SemanticError>;

// Synthetic position values for compiler-generated code (e.g., monomorphization)
pub(super) const SYNTHETIC_LINE: usize = 0;
pub(super) const SYNTHETIC_COL: usize = 0;

#[derive(Clone)]
pub(super) struct LoopContext {
    pub(super) break_patches: Vec<u32>, // Addresses of break jumps to patch
    pub(super) continue_target: u32,    // Address to jump to for continue
}

pub struct ExternSymbolId {
    pub module_path: String,
    pub symbol_id: u32,
}

pub struct Generator {
    pub(super) builder: ByteCodeBuilder,
    pub(super) symbol_table: SymbolTable,
    pub(super) local_scope: Option<LocalSymbolScope>,
    pub(super) pending_monomorphizations:
        Vec<(u32, TypeId, Vec<Statement>, Vec<String>, Vec<Type>)>, // (symbol_id, type_id, body, param_names, params)
    pub(super) module_provider: Box<dyn crate::ModuleProvider>,
    pub(super) imported_modules: std::collections::HashMap<String, Module>,
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
    pub fn new(module_provider: Box<dyn crate::ModuleProvider>) -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
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
            types: self.symbol_table.types.clone(),
            string_constants: self.string_constants.clone(),
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
            match self.compile_module(source) {
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
    pub(super) fn compile_module(&self, source: String) -> SaResult<Module> {
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
                return Err(SemanticError::Other(format!(
                    "Parse error in imported module: {:?}",
                    e
                )));
            }
        };

        // Create a new generator for the imported module with a cloned provider
        let mut module_gen = Generator::new_for_module(self.module_provider.clone_boxed());
        let result = module_gen.generate(statements);
        result
    }

    /// Create a new generator for compiling imported modules (skips builtin preload to avoid recursion)
    fn new_for_module(module_provider: Box<dyn crate::ModuleProvider>) -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
            moved_variables: std::collections::HashSet::new(),
            loop_context_stack: Vec::new(),
            string_constants: Vec::new(),
            current_self_type: None,
            is_compiling_builtin: true, // Skip builtin preload to avoid infinite recursion
        }
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
    pub(super) fn generate_pattern_matching(
        &mut self,
        arms: &[MatchArm],
        scrutinee_ty: Type,
    ) -> SaResult<Type> {
        // Track jump addresses for all arms to jump to end
        let mut end_jumps = Vec::new();
        let mut result_ty = None;

        for (i, arm) in arms.iter().enumerate() {
            let is_last_arm = i == arms.len() - 1;

            // Check if this is a wildcard pattern
            if !arm.pattern.pattern.is_wildcard() {
                // Non-wildcard pattern: need to compare
                self.builder.dup();

                // Generate code to load the pattern value
                let pattern_expr = self.pattern_to_expression(&arm.pattern.pattern)?;
                self.generate_expression(pattern_expr)?;

                // Compare: are they equal?
                self.builder.eq();

                // Negate the result: 1 if not equal, 0 if equal
                self.builder.not();

                // If not equal (result is 1 after not), jump to next arm
                let no_match_jump_placeholder = self.builder.next_address();
                self.builder.jif(0); // Placeholder

                // Pattern matched! Bind to variable if there's a name
                self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                // Generate the expression for this arm
                let arm_ty = self.generate_expression(arm.expression.clone())?;
                self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                // Jump to end of all arms (unless this is the last arm)
                if !is_last_arm {
                    let end_jump_address = self.builder.next_address();
                    self.builder.jmp(0); // Placeholder
                    end_jumps.push(end_jump_address);
                }

                // Patch the no-match jump to point here (next arm)
                let next_arm_address = self.builder.next_address();
                self.builder
                    .patch_jump_address(no_match_jump_placeholder, next_arm_address);
            } else {
                // Wildcard pattern - matches everything
                self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                // Generate the expression for this arm
                let arm_ty = self.generate_expression(arm.expression.clone())?;
                self.validate_match_arm_type(&mut result_ty, arm_ty)?;
            }
        }

        // Patch all end jumps to point here
        let end_address = self.builder.next_address();
        for jump_address in end_jumps {
            self.builder.patch_jump_address(jump_address, end_address);
        }

        Ok(result_ty.unwrap_or(Type::Primitive(PrimitiveType::Unit)))
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

        let type_id = self.symbol_table.add_udt(UserDefinedType::Function(ty));

        // For generic functions, use placeholder address (no bytecode generated)
        // For non-generic functions, capture the address where body generation will start
        let symbol = Symbol::new(
            declaration.name.name,
            qualified_name,
            SymbolKind::Function {
                type_id,
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
                        SymbolKind::Struct(type_id) => Some(Type::Struct(*type_id)),
                        SymbolKind::Enum(type_id) => Some(Type::Enum(*type_id)),
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

    pub(super) fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
        // Extract callee and args so we can inspect callee structure (method vs plain function)
        let callee_expr = *call.callee;
        let arguments = call.arguments;
        let (call_line, call_col) = callee_expr.get_pos();
        let call_name = callee_expr.get_name();

        // Handle generic instantiation: Container<int>(value) or identity<int>(42)
        if let Expression::GenericInstantiation { base, type_args } = &callee_expr {
            // First, try to find the symbol
            let symbol_id = self.symbol_table.find_symbol_in_scope(&base.name).ok_or(
                SemanticError::FunctionNotFound {
                    name: base.name.clone(),
                    pos: Some(SourcePos {
                        line: base.line,
                        col: base.col,
                    }),
                },
            )?;

            let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();
            let symbol_kind = symbol.kind.clone(); // Clone to avoid borrow issues

            match symbol_kind {
                SymbolKind::Function { type_id, .. } => {
                    // This is a generic function call like identity<int>(42)
                    // Resolve all type arguments
                    let mut resolved_type_args = Vec::new();
                    for arg in type_args {
                        resolved_type_args.push(self.get_semantic_type(arg)?);
                    }

                    // Monomorphize the function
                    let (_addr, func_type_id, symbol_id) = self.monomorphize_function(
                        type_id,
                        resolved_type_args,
                        &base.name,
                        base.line,
                        base.col,
                    )?;

                    let function_udt = match self.symbol_table.get_udt(func_type_id) {
                        UserDefinedType::Function(f) => f.clone(),
                        _ => {
                            return Err(SemanticError::FunctionNotFound {
                                name: base.name.clone(),
                                pos: Some(SourcePos {
                                    line: base.line,
                                    col: base.col,
                                }),
                            });
                        }
                    };

                    let param_names: Vec<String> = function_udt.param_names.clone();
                    let param_defaults: Vec<Option<Expression>> =
                        function_udt.param_defaults.clone();
                    let ret_type = function_udt.return_type.clone();

                    // Process arguments
                    let ordered_exprs = self.process_arguments(
                        &base.name,
                        base.line,
                        base.col,
                        arguments,
                        &param_names,
                        &param_defaults,
                    )?;

                    // Generate argument evaluation
                    for expr in ordered_exprs {
                        // Check if this expression involves a move (before consuming it)
                        let move_info = if let Expression::Identifier(ref ident) = expr {
                            if let Some(var_id) = self
                                .local_scope
                                .as_ref()
                                .unwrap()
                                .find_variable(&ident.name)
                            {
                                let ty =
                                    self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                                if !ty.is_copy_treated(&self.symbol_table) {
                                    Some(var_id)
                                } else {
                                    None
                                }
                            } else if let Some(param_id) =
                                self.local_scope.as_ref().unwrap().find_param(&ident.name)
                            {
                                let ty =
                                    self.local_scope.as_ref().unwrap().get_param_type(param_id);
                                if !ty.is_copy_treated(&self.symbol_table) {
                                    Some(param_id)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        self.generate_expression(expr)?;

                        // Mark variable as moved if needed
                        if let Some(var_id) = move_info {
                            self.mark_variable_moved(var_id);
                        }
                    }

                    // Call the monomorphized function using symbol_id
                    self.builder.call(symbol_id);

                    return Ok(ret_type);
                }
                SymbolKind::Struct(_type_id) => {
                    // This is a struct instantiation like Container<int>(value)
                    // Resolve the generic type with explicit type arguments
                    let parsed_type = crate::ast::Type::Generic {
                        base: base.clone(),
                        type_args: type_args.clone(),
                    };

                    // Use monomorphization to get the concrete type
                    let ty = self.get_semantic_type(&parsed_type)?;

                    if let Type::Struct(struct_type_id) = ty {
                        return self.generate_struct_from_call(
                            crate::ast::FunctionCall {
                                callee: Box::new(callee_expr.clone()),
                                arguments,
                            },
                            struct_type_id,
                        );
                    } else {
                        return Err(SemanticError::TypeMismatch {
                            lhs: "struct".to_string(),
                            rhs: ty.to_string(),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                }
                SymbolKind::Enum(_type_id) => {
                    // This is an enum variant construction like Option<int>.Some(42)
                    // Resolve the generic type with explicit type arguments
                    let parsed_type = crate::ast::Type::Generic {
                        base: base.clone(),
                        type_args: type_args.clone(),
                    };

                    // Use monomorphization to get the concrete enum type
                    let ty = self.get_semantic_type(&parsed_type)?;

                    if let Type::Enum(enum_type_id) = ty {
                        return self.generate_enum_variant_from_call(
                            callee_expr.clone(),
                            arguments,
                            enum_type_id,
                        );
                    } else {
                        return Err(SemanticError::TypeMismatch {
                            lhs: "enum".to_string(),
                            rhs: ty.to_string(),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                }
                _ => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "function or struct".to_string(),
                        rhs: format!("symbol kind: {:?}", symbol.kind),
                        pos: Some(SourcePos {
                            line: base.line,
                            col: base.col,
                        }),
                    });
                }
            }
        }

        // Handle field-call (method or static method) specially.
        if let Expression::FieldAccess { object, field } = &callee_expr {
            // Case 1: Static method call or enum variant construction on a generic type
            //         like `Option<int>.Some(...)` or `Type<T>.method(...)`
            if let Expression::GenericInstantiation { base, type_args } = object.as_ref() {
                // This is a generic type access like Option<int>.Some(42)
                // Try to find the base type
                if let Some(base_ty) = self.symbol_table.find_type_in_scope(&base.name) {
                    // Resolve the generic type to its monomorphized version
                    let parsed_type = crate::ast::Type::Generic {
                        base: base.clone(),
                        type_args: type_args.clone(),
                    };
                    let concrete_ty = self.get_semantic_type(&parsed_type)?;

                    // Handle enum variant construction: Option<int>.Some(42)
                    if let Type::Enum(type_id) = concrete_ty {
                        return self.generate_enum_variant_from_call(
                            callee_expr.clone(),
                            arguments,
                            type_id,
                        );
                    }

                    // Handle struct methods on generic structs if needed
                    if let Type::Struct(_type_id) = concrete_ty {
                        // TODO: Handle static methods on generic structs if needed
                    }
                }
            }

            // Case 2: Static method call like `Type.method(...)` (object is identifier referring to a type)
            if let Expression::Identifier(ident) = object.as_ref() {
                if let Some(ty) = self.symbol_table.find_type_in_scope(&ident.name) {
                    if let Type::Struct(_type_id) = ty {
                        // Find the struct symbol and then the method as its child
                        let struct_symbol_id = self
                            .symbol_table
                            .find_symbol_in_scope(&ident.name)
                            .ok_or(SemanticError::FunctionNotFound {
                                name: format!("{}.{}", ident.name, field.name),
                                pos: Some(SourcePos {
                                    line: field.line,
                                    col: field.col,
                                }),
                            })?;

                        let struct_symbol = self.symbol_table.get_symbol(struct_symbol_id).unwrap();
                        let method_symbol_id =
                            struct_symbol.children.get(&field.name).cloned().ok_or(
                                SemanticError::FunctionNotFound {
                                    name: format!("{}.{}", ident.name, field.name),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                },
                            )?;

                        let method_symbol = self.symbol_table.get_symbol(method_symbol_id).unwrap();
                        let (_addr, type_id) = match method_symbol.kind {
                            SymbolKind::Function { address, type_id } => (address, type_id),
                            _ => {
                                return Err(SemanticError::FunctionNotFound {
                                    name: format!("{}.{}", ident.name, field.name),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            }
                        };

                        let function_udt = match self.symbol_table.get_udt(type_id) {
                            UserDefinedType::Function(f) => f.clone(),
                            _ => {
                                return Err(SemanticError::FunctionNotFound {
                                    name: format!("{}.{}", ident.name, field.name),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            }
                        };

                        let param_names: Vec<String> = function_udt.param_names.clone();
                        let param_defaults: Vec<Option<Expression>> =
                            function_udt.param_defaults.clone();
                        let ret_type = function_udt.return_type.clone();

                        // Static method: process all args normally (no receiver pre-pushed)
                        let ordered_exprs = self.process_arguments(
                            &format!("{}.{}", ident.name, field.name),
                            field.line,
                            field.col,
                            arguments,
                            &param_names,
                            &param_defaults,
                        )?;

                        self.push_typed_argument_list(
                            ordered_exprs,
                            &function_udt.params,
                            field.line,
                            field.col,
                        )?;
                        self.builder.call(method_symbol_id);

                        return Ok(ret_type);
                    }
                    // Handle enum variant construction: EnumName.Variant(args)
                    if let Type::Enum(type_id) = ty {
                        return self.generate_enum_variant_from_call(
                            callee_expr.clone(),
                            arguments,
                            type_id,
                        );
                    }
                }
            }

            // Case 3: Instance method call like `obj.method(...)`
            // Generate the object expression first (pushes receiver on stack)
            let object_ty = self.generate_expression(object.as_ref().clone())?;

            // Check if this is a proto box method call
            if let Type::BoxType(inner) = &object_ty {
                if let Type::Primitive(prim_ty) = inner.as_ref() {
                    let ty = self.handle_primitive_method_call(
                        prim_ty, field, &arguments, call_line, call_col,
                    )?;

                    return Ok(ty);
                } else if let Type::Proto(proto_id) = inner.as_ref() {
                    // This is a call to a method on box<Proto> - use dynamic dispatch
                    // The receiver (ProtoBoxRef) is already on the stack

                    // Get the proto definition to find method signature
                    let proto = match &self.symbol_table.types[*proto_id as usize] {
                        UserDefinedType::Proto(p) => p,
                        _ => return Err(SemanticError::Other("Expected proto type".to_string())),
                    };

                    // Find the method in the proto
                    let (method_params, method_return) = proto
                        .methods
                        .iter()
                        .find(|(name, _, _)| name == &field.name)
                        .map(|(_, params, ret)| (params.clone(), ret.clone()))
                        .ok_or_else(|| SemanticError::FunctionNotFound {
                            name: format!("proto method {}", field.name),
                            pos: Some(SourcePos {
                                line: field.line,
                                col: field.col,
                            }),
                        })?;

                    // Process arguments (skip first param which is self)
                    let param_count = method_params.len();
                    if param_count > 0 {
                        // Skip self parameter for argument processing
                        // For now, assume proto methods don't have named params or defaults
                        // Just push the provided arguments in order
                        for arg in arguments {
                            let (_, expr) = arg;
                            self.generate_expression(expr)?;
                        }
                    }

                    // Hash the method name
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    field.name.hash(&mut hasher);
                    let method_hash = hasher.finish() as u32;

                    // Emit CallProtoMethod instruction
                    self.builder
                        .add_instruction(Instruction::CallProtoMethod(*proto_id, method_hash));

                    return Ok(method_return.unwrap_or(Type::Primitive(PrimitiveType::Unit)));
                }
            }

            // Check if this is a proto ref method call
            if let Type::Reference(inner) = &object_ty {
                if let Type::Primitive(prim_ty) = inner.as_ref() {
                    let ty = self.handle_primitive_method_call(
                        prim_ty, field, &arguments, call_line, call_col,
                    )?;

                    return Ok(ty);
                } else if let Type::Proto(proto_id) = inner.as_ref() {
                    // This is a call to a method on ref<Proto> - use dynamic dispatch
                    // The receiver (ProtoRefRef) is already on the stack

                    // Get the proto definition to find method signature
                    let proto = match &self.symbol_table.types[*proto_id as usize] {
                        UserDefinedType::Proto(p) => p,
                        _ => return Err(SemanticError::Other("Expected proto type".to_string())),
                    };

                    // Find the method in the proto
                    let (method_params, method_return) = proto
                        .methods
                        .iter()
                        .find(|(name, _, _)| name == &field.name)
                        .map(|(_, params, ret)| (params.clone(), ret.clone()))
                        .ok_or_else(|| SemanticError::FunctionNotFound {
                            name: format!("proto method {}", field.name),
                            pos: Some(SourcePos {
                                line: field.line,
                                col: field.col,
                            }),
                        })?;

                    // Process arguments (skip first param which is self)
                    let param_count = method_params.len();
                    if param_count > 0 {
                        // Skip self parameter for argument processing
                        // For now, assume proto methods don't have named params or defaults
                        // Just push the provided arguments in order
                        for arg in arguments {
                            let (_, expr) = arg;
                            self.generate_expression(expr)?;
                        }
                    }

                    // Hash the method name
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    field.name.hash(&mut hasher);
                    let method_hash = hasher.finish() as u32;

                    // Emit CallProtoMethod instruction
                    self.builder
                        .add_instruction(Instruction::CallProtoMethod(*proto_id, method_hash));

                    return Ok(method_return.unwrap_or(Type::Primitive(PrimitiveType::Unit)));
                }
            }

            // Resolve the type symbol for method lookup
            // For primitive types like string, look up the primitive type symbol
            // For struct types, look up the struct symbol
            let type_symbol_id = if let Type::Reference(boxed) = &object_ty {
                match boxed.as_ref() {
                    Type::Struct(id) => {
                        // Find the struct symbol corresponding to the TypeId
                        self.symbol_table
                            .symbols
                            .iter()
                            .enumerate()
                            .find(|(_, s)| match s.kind {
                                SymbolKind::Struct(sid) => sid == *id,
                                _ => false,
                            })
                            .map(|(i, _)| i as u32)
                    }
                    _ => None,
                }
            } else if let Type::Struct(id) = &object_ty {
                // Find the struct symbol corresponding to the TypeId
                self.symbol_table
                    .symbols
                    .iter()
                    .enumerate()
                    .find(|(_, s)| match s.kind {
                        SymbolKind::Struct(sid) => sid == *id,
                        _ => false,
                    })
                    .map(|(i, _)| i as u32)
            } else if let Type::Primitive(prim_ty) = &object_ty {
                let ty = self
                    .handle_primitive_method_call(prim_ty, field, &arguments, call_line, call_col);
                return ty;
            } else {
                None
            };

            let symbol_id = type_symbol_id.expect(&format!(
                "Generating method call for type failed: {:?}",
                object_ty
            ));

            let type_symbol = self.symbol_table.get_symbol(symbol_id).unwrap();
            let method_symbol_id = type_symbol.children.get(&field.name).cloned().ok_or(
                SemanticError::FunctionNotFound {
                    name: field.name.clone(),
                    pos: Some(SourcePos {
                        line: field.line,
                        col: field.col,
                    }),
                },
            )?;

            let method_symbol = self.symbol_table.get_symbol(method_symbol_id).unwrap();
            let (_, method_type_id) = match method_symbol.kind {
                SymbolKind::Function { address, type_id } => (address, type_id),
                _ => {
                    return Err(SemanticError::FunctionNotFound {
                        name: field.name.clone(),
                        pos: Some(SourcePos {
                            line: field.line,
                            col: field.col,
                        }),
                    });
                }
            };

            let function_udt = match self.symbol_table.get_udt(method_type_id) {
                UserDefinedType::Function(f) => f.clone(),
                _ => {
                    return Err(SemanticError::FunctionNotFound {
                        name: field.name.clone(),
                        pos: Some(SourcePos {
                            line: field.line,
                            col: field.col,
                        }),
                    });
                }
            };

            // For instance methods the first parameter is the receiver (self) which we've already pushed.
            // So process remaining parameters (skip first).
            let param_names_full: Vec<String> = function_udt.param_names.clone();
            let param_defaults_full: Vec<Option<Expression>> = function_udt.param_defaults.clone();

            if param_names_full.is_empty() {
                // No params declared (we still pushed receiver) - ensure no args provided
                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args expected".to_string(),
                        rhs: format!("{} provided", arguments.len()),
                        pos: Some(SourcePos {
                            line: call_line,
                            col: call_col,
                        }),
                    });
                }

                // Call method (receiver already on stack)
                self.builder.call(method_symbol_id);
                return Ok(function_udt.return_type.clone());
            }

            // Skip receiver param
            let param_names_tail = &param_names_full[1..];
            let param_defaults_tail = &param_defaults_full[1..];

            // Process only the provided arguments (not including receiver)
            let ordered_exprs = self.process_arguments(
                &format!("{}.{}", type_symbol.name, field.name),
                field.line,
                field.col,
                arguments,
                param_names_tail,
                param_defaults_tail,
            )?;

            // This will generate remaining arguments and push them after the receiver.
            self.push_typed_argument_list(
                ordered_exprs,
                &function_udt.params[1..],
                field.line,
                field.col,
            )?;
            self.builder.call(method_symbol_id);

            Ok(function_udt.return_type.clone())
        } else {
            // Non-field callee: top-level function or constructor by name
            let call_name = call_name;

            // Handle box(expr) intrinsic constructor
            if call_name == "box" {
                // box(expr) takes one argument and returns box<T> where T is the type of expr
                if arguments.len() != 1 {
                    return Err(SemanticError::Other(format!(
                        "box() requires exactly one argument, found {} at line {} column {}",
                        arguments.len(),
                        call_line,
                        call_col
                    )));
                }

                // Check if argument is named (not allowed for box)
                let (name_opt, expr) = &arguments[0];
                if name_opt.is_some() {
                    return Err(SemanticError::Other(format!(
                        "box() does not accept named arguments at line {} column {}",
                        call_line, call_col
                    )));
                }

                // Generate code for the argument expression
                let inner_ty = self.generate_expression(expr.clone())?;

                // Generate box allocation instruction
                self.builder.box_alloc();

                // Return box<T> type
                return Ok(Type::BoxType(Box::new(inner_ty)));
            }

            // First try type-name constructor (e.g., Point(...))
            // Handle Self(...) for struct construction inside methods
            if call_name == "Self" {
                if let Some(self_type) = &self.current_self_type {
                    match self_type {
                        Type::Struct(type_id) => {
                            return self.generate_struct_from_call(
                                crate::ast::FunctionCall {
                                    callee: Box::new(Expression::Identifier(
                                        crate::ast::Identifier {
                                            name: "Self".to_string(),
                                            line: call_line,
                                            col: call_col,
                                        },
                                    )),
                                    arguments,
                                },
                                *type_id,
                            );
                        }
                        Type::Enum(type_id) => {
                            // For enums, Self can't be called directly as a constructor
                            // Enums use Self.VariantName(...) syntax
                            return Err(SemanticError::Other(format!(
                                "Self(...) constructor is not valid for enums. Use Self.VariantName(...) at line {} column {}",
                                call_line, call_col
                            )));
                        }
                        _ => {
                            return Err(SemanticError::Other(format!(
                                "Self(...) constructor is only valid for structs at line {} column {}",
                                call_line, call_col
                            )));
                        }
                    }
                } else {
                    return Err(SemanticError::Other(format!(
                        "Self can only be used inside methods at line {} column {}",
                        call_line, call_col
                    )));
                }
            }

            if let Some(ty) = self.symbol_table.find_type_in_scope(&call_name)
                && let Type::Struct(type_id) = ty
            {
                return self.generate_struct_from_call(
                    crate::ast::FunctionCall {
                        callee: Box::new(Expression::Identifier(crate::ast::Identifier {
                            name: call_name.clone(),
                            line: call_line,
                            col: call_col,
                        })),
                        arguments,
                    },
                    type_id,
                );
            }

            if let Some(symbol_id) = self.symbol_table.find_symbol_in_scope(&call_name) {
                let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();

                // Check if this is a struct initialization (struct name used as a function)
                if let SymbolKind::Struct(type_id) = symbol.kind {
                    return self.generate_struct_from_call(
                        crate::ast::FunctionCall {
                            callee: Box::new(Expression::Identifier(crate::ast::Identifier {
                                name: call_name.clone(),
                                line: call_line,
                                col: call_col,
                            })),
                            arguments,
                        },
                        type_id,
                    );
                }

                let (_, type_id) = match symbol.kind {
                    SymbolKind::Function { address, type_id } => (address, type_id),
                    _ => {
                        return Err(SemanticError::FunctionNotFound {
                            name: call_name.clone(),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                };

                let function_udt = match self.symbol_table.get_udt(type_id) {
                    UserDefinedType::Function(function) => function.clone(),
                    _ => {
                        return Err(SemanticError::FunctionNotFound {
                            name: call_name.clone(),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                };

                let param_names: Vec<String> = function_udt.param_names.clone();
                let param_defaults: Vec<Option<Expression>> = function_udt.param_defaults.clone();

                // Use shared argument processing logic
                let ordered_exprs = self.process_arguments(
                    &call_name,
                    call_line,
                    call_col,
                    arguments,
                    &param_names,
                    &param_defaults,
                )?;

                self.push_typed_argument_list(
                    ordered_exprs,
                    &function_udt.params,
                    call_line,
                    call_col,
                )?;
                self.builder.call(symbol_id);

                let ty = self.symbol_table.get_udt(type_id);
                match ty {
                    UserDefinedType::Function(function) => Ok(function.return_type.clone()),
                    _ => panic!("Function not found"),
                }
            } else {
                Err(SemanticError::FunctionNotFound {
                    name: call_name,
                    pos: Some(SourcePos {
                        line: call_line,
                        col: call_col,
                    }),
                })
            }
        }
    }

    /// Process arguments (positional or named) and map them to parameters/fields.
    /// Returns ordered expressions matching the parameter/field order.
    pub(super) fn process_arguments(
        &self,
        call_name: &str,
        call_line: usize,
        call_col: usize,
        arguments: Vec<(Option<Identifier>, Expression)>,
        param_names: &[String],
        param_defaults: &[Option<Expression>],
    ) -> SaResult<Vec<Expression>> {
        let has_named = arguments.iter().any(|(n, _)| n.is_some());
        let has_positional = arguments.iter().any(|(n, _)| n.is_none());

        if has_named && has_positional {
            return Err(SemanticError::MixedNamedAndPositional {
                name: call_name.to_string(),
                pos: Some(SourcePos {
                    line: call_line,
                    col: call_col,
                }),
            });
        }

        let mut ordered_exprs: Vec<Expression> = Vec::with_capacity(param_names.len());

        if has_named {
            // Named arguments: build a map and order by parameters
            let mut arg_map = std::collections::HashMap::new();
            for (name_opt, expr) in arguments.into_iter() {
                if let Some(name) = name_opt {
                    arg_map.insert(name.name, expr);
                }
            }

            // For each parameter, use provided arg or default
            for (i, param_name) in param_names.iter().enumerate() {
                if let Some(expr) = arg_map.remove(param_name) {
                    ordered_exprs.push(expr);
                } else if let Some(default_expr) = param_defaults.get(i).and_then(|o| o.clone()) {
                    ordered_exprs.push(default_expr);
                } else {
                    return Err(SemanticError::TypeMismatch {
                        lhs: param_name.clone(),
                        rhs: "missing required argument".to_string(),
                        pos: Some(SourcePos {
                            line: call_line,
                            col: call_col,
                        }),
                    });
                }
            }

            // Check for unexpected named arguments
            if !arg_map.is_empty() {
                let unexpected = arg_map.keys().next().unwrap().clone();
                return Err(SemanticError::TypeMismatch {
                    lhs: unexpected,
                    rhs: "unexpected argument".to_string(),
                    pos: Some(SourcePos {
                        line: call_line,
                        col: call_col,
                    }),
                });
            }
        } else {
            // Positional arguments
            let provided_count = arguments.len();
            if provided_count > param_names.len() {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!("{} args expected", param_names.len()),
                    rhs: format!("{} provided", provided_count),
                    pos: Some(SourcePos {
                        line: call_line,
                        col: call_col,
                    }),
                });
            }

            // Add provided arguments
            for (_name_opt, expr) in arguments.into_iter() {
                ordered_exprs.push(expr);
            }

            // Fill remaining with defaults
            for i in provided_count..param_names.len() {
                if let Some(default_expr) = param_defaults.get(i).and_then(|o| o.clone()) {
                    ordered_exprs.push(default_expr);
                } else {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("{} args expected", param_names.len()),
                        rhs: format!("{} provided", provided_count),
                        pos: Some(SourcePos {
                            line: call_line,
                            col: call_col,
                        }),
                    });
                }
            }
        }

        Ok(ordered_exprs)
    }

    fn generate_struct_from_call(&mut self, call: FunctionCall, type_id: TypeId) -> SaResult<Type> {
        // Get struct definition
        let (call_name, (call_line, call_col)) = (call.callee.get_name(), call.callee.get_pos());

        let struct_def = match self.symbol_table.get_udt(type_id) {
            UserDefinedType::Struct(s) => s.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "Struct".to_string(),
                    rhs: "Non-struct type".to_string(),
                    pos: Some(SourcePos {
                        line: call_line,
                        col: call_col,
                    }),
                });
            }
        };

        let field_names: Vec<String> = struct_def
            .fields
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let field_defaults: Vec<Option<Expression>> = struct_def.field_defaults.clone();

        // Process arguments using shared logic
        let ordered_exprs = self.process_arguments(
            &call_name,
            call_line,
            call_col,
            call.arguments,
            &field_names,
            &field_defaults,
        )?;

        self.push_argument_list(ordered_exprs)?;
        self.builder.newstruct(struct_def.fields.len() as u32);

        Ok(Type::Struct(type_id))
    }

    fn generate_enum_variant_from_call(
        &mut self,
        callee_expr: Expression,
        arguments: Vec<(Option<Identifier>, Expression)>,
        enum_type_id: TypeId,
    ) -> SaResult<Type> {
        // Extract the variant name from the field access
        let variant_name = if let Expression::FieldAccess { object: _, field } = &callee_expr {
            field.name.clone()
        } else {
            return Err(SemanticError::Other(
                "Invalid enum variant construction".to_string(),
            ));
        };

        // Get enum definition
        let enum_def = match self.symbol_table.get_udt(enum_type_id) {
            UserDefinedType::Enum(e) => e.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "Enum".to_string(),
                    rhs: "Non-enum type".to_string(),
                    pos: Some(SourcePos {
                        line: callee_expr.get_pos().0,
                        col: callee_expr.get_pos().1,
                    }),
                });
            }
        };

        // Find the variant
        let variant_opt = enum_def
            .variants
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == &variant_name);

        let (variant_index, field_types) = if let Some((idx, (_, types))) = variant_opt {
            (idx, types.clone())
        } else {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Enum '{}'", enum_def.qualified_name),
                rhs: format!("Unknown variant '{}'", variant_name),
                pos: Some(SourcePos {
                    line: callee_expr.get_pos().0,
                    col: callee_expr.get_pos().1,
                }),
            });
        };

        // Validate argument count
        if arguments.len() != field_types.len() {
            return Err(SemanticError::TypeMismatch {
                lhs: format!(
                    "{} arguments expected for variant '{}'",
                    field_types.len(),
                    variant_name
                ),
                rhs: format!("{} provided", arguments.len()),
                pos: Some(SourcePos {
                    line: callee_expr.get_pos().0,
                    col: callee_expr.get_pos().1,
                }),
            });
        }

        // Check if this is a payload variant
        if field_types.is_empty() {
            // Unit variant called like a function - this is an error
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Unit variant '{}'", variant_name),
                rhs: "Cannot call unit variant with arguments".to_string(),
                pos: Some(SourcePos {
                    line: callee_expr.get_pos().0,
                    col: callee_expr.get_pos().1,
                }),
            });
        }

        // For payload variants, generate code to create the enum value
        // First, push the variant index
        self.builder.ldi(variant_index as i32);

        // Then push all the field values
        for (arg_opt, expected_type) in arguments.iter().zip(field_types.iter()) {
            let arg_expr = &arg_opt.1;
            let arg_type = self.generate_expression(arg_expr.clone())?;

            // Type check the argument
            if !self.types_compatible(&arg_type, expected_type) {
                return Err(SemanticError::TypeMismatch {
                    lhs: arg_type.to_string(),
                    rhs: expected_type.to_string(),
                    pos: Some(SourcePos {
                        line: callee_expr.get_pos().0,
                        col: callee_expr.get_pos().1,
                    }),
                });
            }
        }

        // Create the enum value with the variant index and fields
        // We represent enum values as structs where the first field is the variant index
        // and subsequent fields are the payload values: [variant_index, field1, field2, ...]
        self.builder.newstruct((field_types.len() + 1) as u32);

        Ok(Type::Enum(enum_type_id))
    }

    fn push_argument_list(&mut self, arguments: Vec<Expression>) -> SaResult<()> {
        for expr in arguments {
            self.generate_expression(expr)?;
        }

        Ok(())
    }

    pub(super) fn push_typed_argument_list(
        &mut self,
        arguments: Vec<Expression>,
        param_types: &[Type],
        call_line: usize,
        call_col: usize,
    ) -> SaResult<()> {
        if arguments.len() != param_types.len() {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("{} args expected", param_types.len()),
                rhs: format!("{} provided", arguments.len()),
                pos: Some(SourcePos {
                    line: call_line,
                    col: call_col,
                }),
            });
        }

        for (expr, param_ty) in arguments.into_iter().zip(param_types.iter()) {
            // Check if this expression involves a move (before consuming it)
            let move_info = if let Expression::Identifier(ref ident) = expr {
                if let Some(var_id) = self
                    .local_scope
                    .as_ref()
                    .unwrap()
                    .find_variable(&ident.name)
                {
                    let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                    if !ty.is_copy_treated(&self.symbol_table) {
                        Some(var_id)
                    } else {
                        None
                    }
                } else if let Some(param_id) =
                    self.local_scope.as_ref().unwrap().find_param(&ident.name)
                {
                    let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                    if !ty.is_copy_treated(&self.symbol_table) {
                        Some(param_id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let arg_ty = self.generate_expression(expr)?;

            // Mark variable as moved if needed
            if let Some(var_id) = move_info {
                self.mark_variable_moved(var_id);
            }

            match (param_ty, &arg_ty) {
                (Type::Reference(param_inner), Type::Reference(arg_inner)) => {
                    if **param_inner != **arg_inner {
                        return Err(SemanticError::TypeMismatch {
                            lhs: arg_ty.to_string(),
                            rhs: param_ty.to_string(),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                }
                (Type::Reference(_), _) => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: arg_ty.to_string(),
                        rhs: param_ty.to_string(),
                        pos: Some(SourcePos {
                            line: call_line,
                            col: call_col,
                        }),
                    });
                }
                (_, Type::Reference(_)) => {
                    // No implicit deref: `ref` values cannot be passed to non-ref parameters.
                    return Err(SemanticError::TypeMismatch {
                        lhs: arg_ty.to_string(),
                        rhs: param_ty.to_string(),
                        pos: Some(SourcePos {
                            line: call_line,
                            col: call_col,
                        }),
                    });
                }
                _ => {
                    // Check type compatibility for non-ref parameters
                    if !self.types_compatible(&arg_ty, param_ty) {
                        return Err(SemanticError::TypeMismatch {
                            lhs: format!("argument type {}", arg_ty.to_string()),
                            rhs: format!("parameter type {}", param_ty.to_string()),
                            pos: Some(SourcePos {
                                line: call_line,
                                col: call_col,
                            }),
                        });
                    }
                }
            }
        }

        Ok(())
    }
}
