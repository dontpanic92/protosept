use crate::errors::SourcePos;
use crate::{
    ast::{
        Expression, FunctionCall, FunctionDeclaration, Identifier, Pattern, Statement,
        Type as ParsedType,
    },
    bytecode::builder::ByteCodeBuilder,
    lexer::TokenType,
    semantic::{
        Enum, Function, LocalSymbolScope, PrimitiveType, Proto, Struct, Symbol, SymbolKind,
        SymbolTable, Type, TypeId, UserDefinedType, Variable,
    },
};

use super::Module;

use crate::errors::SemanticError;

pub type SaResult<T> = Result<T, SemanticError>;

// Synthetic position values for compiler-generated code (e.g., monomorphization)
const SYNTHETIC_LINE: usize = 0;
const SYNTHETIC_COL: usize = 0;

pub struct Generator<'a> {
    builder: ByteCodeBuilder,
    symbol_table: SymbolTable,
    local_scope: Option<LocalSymbolScope>,
    pending_monomorphizations: Vec<(u32, TypeId, Vec<Statement>, Vec<String>, Vec<Type>)>, // (symbol_id, type_id, body, param_names, params)
    module_provider: &'a dyn crate::ModuleProvider,
    imported_modules: std::collections::HashMap<String, Module>,
}

impl<'a> Generator<'a> {
    pub fn new(module_provider: &'a dyn crate::ModuleProvider) -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
            pending_monomorphizations: Vec::new(),
            module_provider,
            imported_modules: std::collections::HashMap::new(),
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
                    if let SymbolKind::Function { address: ref mut func_addr, .. } = sym.kind {
                        *func_addr = address;
                    }
                }
                
                // Set up local scope and generate body
                let variables: Vec<Variable> = param_names.iter()
                    .zip(params.iter())
                    .map(|(name, ty)| Variable {
                        name: name.clone(),
                        ty: ty.clone(),
                    })
                    .collect();
                
                let saved_local_scope = self.local_scope.take();
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
        })
    }

    /// Helper method to compile an imported module
    fn compile_module(&self, source: String) -> SaResult<Module> {
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
        let statements = parser.parse().map_err(|e| 
            SemanticError::Other(format!("Parse error in imported module: {:?}", e))
        )?;

        // Create a new generator for the imported module with the same provider
        let mut module_gen = Generator::new(self.module_provider);
        module_gen.generate(statements)
    }

    fn generate_block(
        &mut self,
        statements: Vec<Statement>,
        variables: Vec<Variable>,
    ) -> SaResult<Type> {
        self.local_scope.as_mut().unwrap().push_scope();
        for var in variables {
            self.local_scope
                .as_mut()
                .unwrap()
                .add_variable(var.name, var.ty)
                .unwrap();
        }

        let mut ty = Type::Primitive(PrimitiveType::Unit);

        // Check if this block contains Branch statements (pattern matching)
        let has_branches = statements
            .iter()
            .any(|s| matches!(s, Statement::Branch { .. }));

        if has_branches {
            // Special handling for blocks with pattern matching branches
            let mut end_jumps = Vec::new();

            for statement in statements {
                if let Statement::Branch { .. } = statement {
                    // Generate the branch, collecting end jump addresses
                    ty = self.generate_branch_statement(statement, &mut end_jumps)?;
                } else {
                    ty = self.generate_statement(statement)?;
                }
            }

            // Patch all end jumps to point here
            let end_address = self.builder.next_address();
            for jump_address in end_jumps {
                self.builder.patch_jump_address(jump_address, end_address);
            }
        } else {
            // Normal block handling
            for statement in statements {
                ty = self.generate_statement(statement)?;
            }
        }

        self.local_scope.as_mut().unwrap().pop_scope();

        Ok(ty)
    }

    fn pattern_to_expression(&self, pattern: &Pattern) -> SaResult<Expression> {
        match pattern {
            Pattern::Identifier(id) => Ok(Expression::Identifier(id.clone())),
            Pattern::IntegerLiteral(val) => Ok(Expression::IntegerLiteral(*val)),
            Pattern::FloatLiteral(val) => Ok(Expression::FloatLiteral(*val)),
            Pattern::StringLiteral(val) => Ok(Expression::StringLiteral(val.clone())),
            Pattern::BooleanLiteral(val) => Ok(Expression::BooleanLiteral(*val)),
            Pattern::FieldAccess { object, field } => {
                let obj_expr = self.pattern_to_expression(object)?;
                Ok(Expression::FieldAccess {
                    object: Box::new(obj_expr),
                    field: field.clone(),
                })
            }
        }
    }

    fn generate_branch_statement(
        &mut self,
        statement: Statement,
        end_jumps: &mut Vec<u32>,
    ) -> SaResult<Type> {
        if let Statement::Branch {
            named_pattern,
            expression,
        } = statement
        {
            // The exception value is on top of the stack (already unwrapped)

            match &named_pattern.pattern {
                Pattern::FieldAccess { object, field } => {
                    // Enum variant pattern like "SomeErrors.NumberIsNot42"

                    // Duplicate the exception value for comparison
                    self.builder.dup();

                    // Generate code to load the enum variant index
                    let pattern_expr = self.pattern_to_expression(&named_pattern.pattern)?;
                    self.generate_expression(pattern_expr)?;

                    // Compare: are they equal?
                    self.builder.eq();

                    // Negate the result: 1 if not equal, 0 if equal
                    self.builder.not();

                    // If not equal (result is 1 after not), jump to next branch
                    let no_match_jump_placeholder = self.builder.next_address();
                    self.builder.jif(0); // Placeholder

                    // Pattern matched!
                    // Bind to variable if there's a name
                    if let Some(name) = &named_pattern.name {
                        let var_id = self
                            .local_scope
                            .as_mut()
                            .unwrap()
                            .add_variable(name.name.clone(), Type::Primitive(PrimitiveType::Int))
                            .map_err(|_| SemanticError::VariableOutsideFunction {
                                name: name.name.clone(),
                                pos: Some(SourcePos {
                                    line: name.line,
                                    col: name.col,
                                }),
                            })?;
                        self.builder.stvar(var_id);
                    } else {
                        // No name binding, pop the exception value
                        self.builder.pop();
                    }

                    // Generate the expression for this branch
                    let expr_type = self.generate_expression(expression)?;

                    // Jump to end of all branches
                    let end_jump_address = self.builder.next_address();
                    self.builder.jmp(0); // Placeholder
                    end_jumps.push(end_jump_address);

                    // Patch the no-match jump to point here (next branch)
                    let next_branch_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(no_match_jump_placeholder, next_branch_address);

                    Ok(expr_type)
                }
                Pattern::Identifier(identifier) => {
                    // Wildcard pattern (_) - matches everything
                    // This is typically the last branch, no jump needed

                    // Bind to variable if named
                    if identifier.name == "_" {
                        // Wildcard, just pop the exception value if not bound
                        if named_pattern.name.is_none() {
                            self.builder.pop();
                        }
                    }

                    if let Some(name) = &named_pattern.name {
                        let var_id = self
                            .local_scope
                            .as_mut()
                            .unwrap()
                            .add_variable(name.name.clone(), Type::Primitive(PrimitiveType::Int))
                            .map_err(|_| SemanticError::VariableOutsideFunction {
                                name: name.name.clone(),
                                pos: Some(SourcePos {
                                    line: name.line,
                                    col: name.col,
                                }),
                            })?;
                        self.builder.stvar(var_id);
                    } else if identifier.name != "_" {
                        // Not a wildcard and not a named binding - this is an error in the pattern
                        return Err(SemanticError::Other(format!(
                            "Identifier pattern '{}' must be '_' or bound to a name",
                            identifier.name
                        )));
                    }

                    // Generate the expression
                    let expr_type = self.generate_expression(expression)?;

                    Ok(expr_type)
                }
                Pattern::IntegerLiteral(_)
                | Pattern::FloatLiteral(_)
                | Pattern::StringLiteral(_)
                | Pattern::BooleanLiteral(_) => {
                    return Err(SemanticError::Other(
                        "Pattern matching for literal patterns not yet implemented".to_string(),
                    ));
                }
            }
        } else {
            // Not a branch statement, use regular handling
            self.generate_statement(statement)
        }
    }

    fn generate_statement(&mut self, statement: Statement) -> SaResult<Type> {
        match statement {
            Statement::Let {
                identifier,
                expression,
            } => {
                let ty = self.generate_expression(expression)?;
                let var_id = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .add_variable(identifier.name.clone(), ty)
                    .map_err(|_| SemanticError::VariableOutsideFunction {
                        name: identifier.name.clone(),
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })?;

                self.builder.stvar(var_id);
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Expression(expression) => self.generate_expression(expression),
            Statement::FunctionDeclaration(declaration) => {
                self.process_function_declaration(declaration)?;

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Throw(expression) => {
                self.generate_expression(expression)?;
                self.builder.throw();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::EnumDeclaration {
                is_pub,
                name,
                attributes,
                type_parameters: _,
                values,
            } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                let ty = Enum {
                    qualified_name: qualified_name.clone(),
                    values: values.iter().map(|v| v.name.clone()).collect(),
                    attributes: attributes.clone(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Enum(ty));

                let symbol =
                    Symbol::new(name.name.clone(), qualified_name, SymbolKind::Enum(type_id));

                let next_symbol_id = self.symbol_table.symbols.len() as u32;
                let current_symbol = self.symbol_table.get_current_symbol_mut().unwrap();
                current_symbol.children.insert(name.name, next_symbol_id);
                self.symbol_table.symbols.push(symbol);

                // TODO: Store is_pub for module visibility checking
                let _ = is_pub;

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::StructDeclaration {
                is_pub,
                name,
                attributes,
                type_parameters,
                fields,
                methods,
            } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                
                // Extract type parameter names
                let type_param_names: Vec<String> = type_parameters
                    .iter()
                    .map(|tp| tp.name.name.clone())
                    .collect();
                
                let is_generic = !type_param_names.is_empty();
                
                // For generic structs, store parsed field types; for non-generic, resolve them
                let (fields_with_types, generic_field_types) = if is_generic {
                    // For generic structs, store placeholder types and keep parsed types
                    let parsed_field_types: Vec<crate::ast::Type> = fields
                        .iter()
                        .map(|f| f.field_type.clone())
                        .collect();
                    
                    // Use Unit as placeholder - these will be properly typed during monomorphization
                    let placeholder_fields: Vec<(String, Type)> = fields
                        .iter()
                        .map(|f| (f.name.name.clone(), Type::Primitive(PrimitiveType::Unit)))
                        .collect();
                    
                    (placeholder_fields, Some(parsed_field_types))
                } else {
                    // For non-generic structs, resolve types normally
                    let mut resolved_fields = Vec::new();
                    for f in &fields {
                        let field_type = self.get_semantic_type(&f.field_type)?;
                        resolved_fields.push((f.name.name.clone(), field_type));
                    }
                    (resolved_fields, None)
                };
                
                let field_defaults = fields.iter().map(|f| f.default_value.clone()).collect();

                let ty = Struct {
                    qualified_name: qualified_name.clone(),
                    fields: fields_with_types,
                    field_defaults,
                    attributes: attributes.clone(),
                    type_parameters: type_param_names,
                    generic_field_types,
                    monomorphization: None,  // This is the generic definition, not a monomorphization
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Struct(ty));

                let symbol = Symbol::new(name.name, qualified_name, SymbolKind::Struct(type_id));
                self.symbol_table.push_symbol(symbol);

                for method in methods {
                    self.process_function_declaration(method.function)?;
                }
                
                // TODO: Store is_pub for module visibility checking
                let _ = is_pub;

                self.symbol_table.pop_symbol();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::ProtoDeclaration {
                is_pub,
                name,
                attributes,
                methods,
            } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                
                // First add the proto to the symbol table as a forward declaration
                // so that method parameters can reference it
                let ty = Proto {
                    qualified_name: qualified_name.clone(),
                    methods: vec![],
                    attributes: attributes.clone(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Proto(ty));
                
                let symbol = Symbol::new(name.name.clone(), qualified_name.clone(), SymbolKind::Proto(type_id));
                self.symbol_table.push_symbol(symbol);
                
                // Now process the method signatures
                let mut methods_with_types = Vec::new();
                for m in methods {
                    let mut params = Vec::new();
                    for p in &m.parameters {
                        params.push(self.get_semantic_type(&p.arg_type)?);
                    }
                    let return_type = match &m.return_type {
                        Some(t) => Some(self.get_semantic_type(t)?),
                        None => None,
                    };
                    methods_with_types.push((m.name.name.clone(), params, return_type));
                }

                // Update the proto with the actual method signatures
                let ty = Proto {
                    qualified_name: qualified_name.clone(),
                    methods: methods_with_types,
                    attributes: attributes.clone(),
                };
                self.symbol_table.types[type_id as usize] = UserDefinedType::Proto(ty);
                
                self.symbol_table.pop_symbol();
                
                // TODO: Store is_pub for module visibility checking
                let _ = is_pub;

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Branch {
                named_pattern,
                expression,
            } => {
                // Branch statements should be handled by generate_branch_statement
                // when they appear in a pattern matching context.
                // If we reach here, it's an error - branches should only appear in try-else blocks
                return Err(SemanticError::Other(
                    "Branch statements can only appear in try-else blocks".to_string(),
                ));
            }
            Statement::Return(expression) => {
                let ty = self.generate_expression(*expression)?;
                if matches!(ty, Type::Reference(_)) {
                    return Err(SemanticError::Other(
                        "Cannot return a non-escapable ref value".to_string(),
                    ));
                }
                self.builder.ret();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Import { module_path, alias } => {
                // Load the module from the module provider
                let source = self.module_provider.load_module(&module_path)
                    .ok_or_else(|| SemanticError::ImportError {
                        module_path: module_path.clone(),
                        pos: SourcePos {
                            line: 0,
                            col: 0,
                        },
                    })?;

                // Compile the imported module if not already compiled
                if !self.imported_modules.contains_key(&module_path) {
                    // Recursively compile the imported module
                    let imported_module = self.compile_module(source)?;
                    self.imported_modules.insert(module_path.clone(), imported_module);
                }

                // Get the binding name (use alias if provided, otherwise last segment)
                let binding_name = if let Some(ref alias_name) = alias {
                    alias_name.clone()
                } else {
                    // Extract last segment from module path (e.g., "test.test" -> "test")
                    module_path
                        .split('.')
                        .last()
                        .unwrap_or(&module_path)
                        .to_string()
                };

                // Import public symbols from the module into the symbol table
                let imported_module = self.imported_modules.get(&module_path).unwrap();
                for symbol in &imported_module.symbols {
                    // Only import public (pub) symbols
                    // For now, we import all symbols from the module as we don't track visibility yet
                    // Create a prefixed symbol name: binding_name.symbol_name
                    let prefixed_name = format!("{}.{}", binding_name, symbol.name);
                    let qualified_prefixed = format!("{}.{}", binding_name, symbol.qualified_name);
                    
                    // Add the symbol to our symbol table with the prefixed name
                    let new_symbol = Symbol::new(
                        prefixed_name,
                        qualified_prefixed,
                        symbol.kind.clone(),
                    );
                    self.symbol_table.symbols.push(new_symbol);
                }

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
        }
    }

    fn generate_expression(&mut self, expression: Expression) -> SaResult<Type> {
        match expression {
            Expression::Identifier(identifier) => {
                if let Some(var_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_variable(&identifier.name)
                {
                    self.builder.ldvar(var_id);
                    let ty = self.local_scope.as_mut().unwrap().get_variable_type(var_id);
                    Ok(ty)
                } else if let Some(param_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_param(&identifier.name)
                {
                    self.builder.ldpar(param_id);
                    let ty = self.local_scope.as_mut().unwrap().get_param_type(param_id);
                    Ok(ty)
                } else {
                    Err(SemanticError::VariableNotFound {
                        name: identifier.name,
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })
                }
            }
            Expression::IntegerLiteral(value) => {
                self.builder.ldi(value as i32);
                Ok(Type::Primitive(PrimitiveType::Int))
            }
            Expression::FloatLiteral(value) => {
                self.builder.ldf(value);
                Ok(Type::Primitive(PrimitiveType::Float))
            }
            Expression::StringLiteral(value) => {
                unimplemented!();
            }
            Expression::BooleanLiteral(value) => {
                self.builder.ldi(if value { 1 } else { 0 });
                Ok(Type::Primitive(PrimitiveType::Bool))
            }
            Expression::Unary { operator, right } => {
                let ty = self.generate_expression(*right)?;
                match operator.token_type {
                    TokenType::Minus => {
                        self.builder.neg();
                        Ok(ty)
                    }
                    TokenType::Not => {
                        self.builder.not();
                        Ok(ty)
                    }
                    TokenType::Multiply => {
                        // `*r` where `r: ref T` yields a `T`. No runtime op yet.
                        if let Type::Reference(inner) = ty {
                            Ok(*inner)
                        } else {
                            Err(SemanticError::TypeMismatch {
                                lhs: ty.to_string(),
                                rhs: "ref <T>".to_string(),
                                pos: Some(SourcePos {
                                    line: operator.line,
                                    col: operator.col,
                                }),
                            })
                        }
                    }
                    _ => unimplemented!(),
                }
            }
            Expression::Ref(identifier) => {
                // `ref x` produces a `ref T` typed value (view).
                // Lowering is currently just loading the underlying slot value.
                if let Some(var_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_variable(&identifier.name)
                {
                    self.builder.ldvar(var_id);
                    let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                    if matches!(ty, Type::Reference(_)) {
                        return Err(SemanticError::Other(format!(
                            "Cannot take ref of ref '{}'",
                            identifier.name
                        )));
                    }
                    Ok(Type::Reference(Box::new(ty)))
                } else if let Some(param_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_param(&identifier.name)
                {
                    self.builder.ldpar(param_id);
                    let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                    if matches!(ty, Type::Reference(_)) {
                        return Err(SemanticError::Other(format!(
                            "Cannot take ref of ref parameter '{}'",
                            identifier.name
                        )));
                    }
                    Ok(Type::Reference(Box::new(ty)))
                } else {
                    Err(SemanticError::VariableNotFound {
                        name: identifier.name,
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })
                }
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => {
                // Handle assignment specially: generate RHS first and then store into the LHS target
                if operator.token_type == TokenType::Assignment {
                    // Handle LHS without generating its value (we need the target)
                    match *left {
                        Expression::Identifier(identifier) => {
                            let _rhs_ty = self.generate_expression(*right)?;

                            // Prefer local variable, fallback to parameter
                            if let Some(var_id) = self
                                .local_scope
                                .as_mut()
                                .unwrap()
                                .find_variable(&identifier.name)
                            {
                                // `ref` is read-only: disallow assignment to ref locals.
                                if matches!(
                                    self.local_scope.as_ref().unwrap().get_variable_type(var_id),
                                    Type::Reference(_)
                                ) {
                                    return Err(SemanticError::Other(format!(
                                        "Cannot assign to read-only ref '{}'",
                                        identifier.name
                                    )));
                                }

                                self.builder.stvar(var_id);
                                return Ok(Type::Primitive(PrimitiveType::Unit));
                            } else if let Some(param_id) = self
                                .local_scope
                                .as_mut()
                                .unwrap()
                                .find_param(&identifier.name)
                            {
                                // `ref` is read-only: disallow assignment to ref parameters.
                                if matches!(
                                    self.local_scope.as_ref().unwrap().get_param_type(param_id),
                                    Type::Reference(_)
                                ) {
                                    return Err(SemanticError::Other(format!(
                                        "Cannot assign to read-only ref parameter '{}'",
                                        identifier.name
                                    )));
                                }

                                // Store into parameter slot (no separate stpar instruction exists;
                                // emit Stvar to simplify codegen — runtime layout may treat params differently)
                                self.builder.stvar(param_id);
                                return Ok(Type::Primitive(PrimitiveType::Unit));
                            } else {
                                return Err(SemanticError::VariableNotFound {
                                    name: identifier.name,
                                    pos: Some(SourcePos {
                                        line: identifier.line,
                                        col: identifier.col,
                                    }),
                                });
                            }
                        }
                        Expression::FieldAccess { object, field } => {
                            let object_ty = self.generate_expression(*object.clone())?;
                            let _rhs_ty = self.generate_expression(*right.clone())?;

                            // `ref` is read-only: disallow `ref_struct.field = ...`.
                            if matches!(object_ty, Type::Reference(_)) {
                                return Err(SemanticError::Other(format!(
                                    "Cannot assign through read-only ref '{}.{}'",
                                    object.get_name(),
                                    field.name
                                )));
                            }

                            if let Type::Struct(type_id) = object_ty {
                                let udt = self.symbol_table.get_udt(type_id);
                                if let UserDefinedType::Struct(struct_def) = udt {
                                    if let Some((idx, (_fname, _ftype))) = struct_def
                                        .fields
                                        .iter()
                                        .enumerate()
                                        .find(|(_i, (fname, _))| fname == &field.name)
                                    {
                                        // Stack is now [object][value], as required by stfield
                                        self.builder.stfield(idx as u32);
                                        return Ok(Type::Primitive(PrimitiveType::Unit));
                                    } else {
                                        return Err(SemanticError::TypeMismatch {
                                            lhs: format!(
                                                "Struct instance '{}: {}'",
                                                object.get_name(),
                                                struct_def.qualified_name
                                            ),
                                            rhs: format!(
                                                "Unknown field '.{}' on struct",
                                                field.name
                                            ),
                                            pos: Some(SourcePos {
                                                line: field.line,
                                                col: field.col,
                                            }),
                                        });
                                    }
                                } else {
                                    unimplemented!(
                                        "Internal error: Type ID resolved to non-Struct UDT"
                                    );
                                }
                            }

                            return Err(SemanticError::TypeMismatch {
                                lhs: object_ty.to_string(),
                                rhs: "Struct instance".to_string(),
                                pos: Some(SourcePos {
                                    line: field.line,
                                    col: field.col,
                                }),
                            });
                        }
                        _ => {
                            // Other lvalues (like indexing, deref) not supported yet.
                            unimplemented!("assignment to this lvalue is not implemented");
                        }
                    }
                }

                // Non-assignment binary operations follow the previous ordering: evaluate LHS then RHS.
                let lhs_ty = self.generate_expression(*left)?;
                let rhs_ty = self.generate_expression(*right)?;
                let result_ty = if lhs_ty == rhs_ty {
                    lhs_ty.clone()
                } else {
                    // Allow implicit int <-> float promotion for arithmetic/comparison.
                    match (&lhs_ty, &rhs_ty) {
                        (
                            Type::Primitive(PrimitiveType::Int),
                            Type::Primitive(PrimitiveType::Float),
                        )
                        | (
                            Type::Primitive(PrimitiveType::Float),
                            Type::Primitive(PrimitiveType::Int),
                        ) => Type::Primitive(PrimitiveType::Float),
                        _ => {
                            return Err(SemanticError::TypeMismatch {
                                lhs: lhs_ty.to_string(),
                                rhs: rhs_ty.to_string(),
                                pos: Some(SourcePos {
                                    line: operator.line,
                                    col: operator.col,
                                }),
                            });
                        }
                    }
                };

                let is_comparison = matches!(
                    operator.token_type,
                    TokenType::Equals
                        | TokenType::NotEquals
                        | TokenType::GreaterThan
                        | TokenType::GreaterThanOrEqual
                        | TokenType::LessThan
                        | TokenType::LessThanOrEqual
                );

                match operator.token_type {
                    TokenType::Plus => self.builder.addi(),
                    TokenType::Minus => self.builder.subi(),
                    TokenType::Multiply => self.builder.muli(),
                    TokenType::Divide => self.builder.divi(),
                    TokenType::And => self.builder.and(),
                    TokenType::Or => self.builder.or(),
                    TokenType::Equals => self.builder.eq(),
                    TokenType::NotEquals => self.builder.neq(),
                    TokenType::GreaterThan => self.builder.gt(),
                    TokenType::GreaterThanOrEqual => self.builder.gte(),
                    TokenType::LessThan => self.builder.lt(),
                    TokenType::LessThanOrEqual => self.builder.lte(),
                    _ => {
                        unimplemented!();
                    }
                };
                if is_comparison {
                    Ok(Type::Primitive(PrimitiveType::Bool))
                } else {
                    Ok(result_ty)
                }
            }
            Expression::If {
                condition,
                then_branch,
                else_branch,
                pos,
            } => {
                let condition_type = self.generate_expression(*condition)?;
                if condition_type != Type::Primitive(PrimitiveType::Bool) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: condition_type.to_string(),
                        rhs: "bool".to_string(),
                        pos: Some(SourcePos {
                            line: pos.0,
                            col: pos.1,
                        }),
                    });
                }

                self.builder.not();
                let jump_if_false_address_placeholder = self.builder.next_address();
                self.builder.jif(0);

                self.generate_expression(*then_branch)?;

                if let Some(else_branch) = else_branch {
                    let jump_to_skip_else_placeholder = self.builder.next_address();
                    self.builder.jmp(0);

                    let else_branch_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(jump_if_false_address_placeholder, else_branch_address);

                    self.generate_expression(*else_branch)?;

                    let end_of_if_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(jump_to_skip_else_placeholder, end_of_if_address);
                } else {
                    let end_of_if_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(jump_if_false_address_placeholder, end_of_if_address);
                }

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Expression::FunctionCall(call) => self.generate_function_call(call),
            Expression::FieldAccess { object, field } => {
                let object_name = object.get_name();
                let (object_ty, is_static_access) =
                    if let Expression::Identifier(ref identifier) = *object {
                        if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
                            (ty, true) // It's a type (static) access
                        } else {
                            // Not a type, so it must be a variable/expression
                            (self.generate_expression(*object)?, false)
                        }
                    } else {
                        // Not an identifier, so definitely an expression
                        (self.generate_expression(*object)?, false)
                    };

                let object_ty = match object_ty {
                    Type::Reference(inner) => *inner,
                    other => other,
                };

                match object_ty {
                    Type::Enum(type_id) => {
                        let udt = self.symbol_table.get_udt(type_id);
                        if let UserDefinedType::Enum(enum_def) = udt {
                            if is_static_access {
                                if enum_def.values.contains(&field.name) {
                                    let variant_index = enum_def
                                        .values
                                        .iter()
                                        .position(|v| v == &field.name)
                                        .unwrap()
                                        as i32;
                                    self.builder.ldi(variant_index); // Load the enum variant's index
                                    Ok(object_ty) // The type of the field access is the enum itself
                                } else {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!("Enum '{}'", enum_def.qualified_name),
                                        rhs: format!("Unknown Enum variant '{}'", field.name),
                                        pos: Some(SourcePos {
                                            line: field.line,
                                            col: field.col,
                                        }),
                                    });
                                }
                            } else {
                                // Field access on an enum instance (e.g., my_enum_var.Variant) is not supported for simple enums.
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Enum instance '{}'", enum_def.qualified_name),
                                    rhs: format!(
                                        "Field access on Enum instance via variant '{}'",
                                        field.name
                                    ),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            }
                        } else {
                            unimplemented!("Internal error: Type ID resolved to non-Enum UDT");
                        }
                    }
                    Type::Struct(type_id) => {
                        let udt = self.symbol_table.get_udt(type_id);
                        if let UserDefinedType::Struct(struct_def) = udt {
                            if is_static_access {
                                // Static field access on Structs not yet supported.
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Struct type '{}'", struct_def.qualified_name),
                                    rhs: format!(
                                        "Static field access on Struct type '{}' (not supported)",
                                        field.name
                                    ),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            } else {
                                // Instance field access: generate code to load the requested field.
                                // First find the field index and type in the struct definition.
                                if let Some((idx, (_fname, ftype))) = struct_def
                                    .fields
                                    .iter()
                                    .enumerate()
                                    .find(|(_i, (fname, _))| fname == &field.name)
                                {
                                    // At runtime, the object expression will push the struct value onto the stack.
                                    // Emit instruction to load the field by index.
                                    self.builder.ldfield(idx as u32);
                                    return Ok(ftype.clone());
                                } else {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!(
                                            "Struct instance '{}: {}'",
                                            object_name, struct_def.qualified_name
                                        ),
                                        rhs: format!("Unknown field '.{}' on struct", field.name),
                                        pos: Some(SourcePos {
                                            line: field.line,
                                            col: field.col,
                                        }),
                                    });
                                }
                            }
                        } else {
                            unimplemented!("Internal error: Type ID resolved to non-Struct UDT");
                        }
                    }
                    _ => {
                        // Primitives, Arrays, References cannot be field accessed via '.' syntax
                        return Err(SemanticError::TypeMismatch {
                            lhs: object_ty.to_string(),
                            rhs: "Struct or Enum type/instance".to_string(),
                            pos: Some(SourcePos {
                                line: field.line,
                                col: field.col,
                            }),
                        });
                    }
                }
            }
            Expression::Block(statements) => self.generate_block(statements, vec![]),
            Expression::Try {
                try_block,
                else_block,
            } => {
                // Generate the try block expression
                let ty = self.generate_expression(*try_block)?;

                if let Some(else_block) = else_block {
                    // After try_block, check if result is an exception
                    // If exception, jump to else_block handler
                    let check_exception_placeholder = self.builder.next_address();
                    self.builder.check_exception(0); // Placeholder address

                    // Normal path: no exception, jump over else block
                    let jump_over_else_placeholder = self.builder.next_address();
                    self.builder.jmp(0); // Placeholder address

                    // Exception handler starts here
                    let else_block_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(check_exception_placeholder, else_block_address);

                    // Unwrap the exception value for the else block to use
                    self.builder.unwrap_exception();

                    // Check if else_block has pattern matching (Branch statements)
                    let has_pattern_matching =
                        if let Expression::Block(statements) = else_block.as_ref() {
                            statements
                                .iter()
                                .any(|s| matches!(s, Statement::Branch { .. }))
                        } else {
                            false
                        };

                    // If no pattern matching, pop the exception value since we won't use it
                    if !has_pattern_matching {
                        self.builder.pop();
                    }

                    // Generate the else block (which may contain Statement::Branch for pattern matching)
                    self.generate_expression(*else_block)?;

                    // End of exception handler
                    let end_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(jump_over_else_placeholder, end_address);
                } else {
                    // No else block - if there's an exception, it propagates
                    // The exception is already on the stack, so it will return to caller
                }

                Ok(ty)
            }
            Expression::BlockValue(expression) => {
                let ty = self.generate_expression(*expression)?;
                Ok(ty)
            }
            Expression::GenericInstantiation { base, .. } => {
                // GenericInstantiation is only valid as a callee in function calls (struct construction)
                // It's not a standalone expression that can be evaluated
                Err(SemanticError::TypeMismatch {
                    lhs: "expression value".to_string(),
                    rhs: format!("generic type instantiation '{}'", base.name),
                    pos: Some(SourcePos {
                        line: base.line,
                        col: base.col,
                    }),
                })
            }
        }
    }

    fn process_function_declaration(&mut self, declaration: FunctionDeclaration) -> SaResult<()> {
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

        let ty = Function {
            qualified_name: qualified_name.clone(),
            params: params.iter().map(|var| var.ty.clone()).collect(),
            param_names: params.iter().map(|var| var.name.clone()).collect(),
            param_defaults: param_defaults.clone(),
            return_type,
            attributes: declaration.attributes.clone(),
            type_parameters: type_param_names.clone(),
            generic_param_types,
            generic_return_type,
            generic_body: if is_generic { Some(declaration.body.clone()) } else { None },
            monomorphization: None,  // This is the generic definition, not a monomorphization
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
            self.local_scope = Some(LocalSymbolScope::new(params.clone()));
            let ty = self.generate_block(declaration.body, vec![])?;
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

    fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
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
            let symbol_kind = symbol.kind.clone();  // Clone to avoid borrow issues
            
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
                    let param_defaults: Vec<Option<Expression>> = function_udt.param_defaults.clone();
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
                        self.generate_expression(expr)?;
                    }
                    
                    // Call the monomorphized function using symbol_id
                    self.builder.call(symbol_id);
                    
                    return Ok(ret_type);
                }
                SymbolKind::Struct(type_id) => {
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
        if let Expression::FieldAccess { object, field } = callee_expr {
            // Case 1: Static method call like `Type.method(...)` (object is identifier referring to a type)
            if let Expression::Identifier(ident) = *object.clone() {
                if let Some(ty) = self.symbol_table.find_type_in_scope(&ident.name) {
                    if let Type::Struct(type_id) = ty {
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
                        let (addr, type_id) = match method_symbol.kind {
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
                }
            }

            // Case 2: Instance method call like `obj.method(...)`
            // Generate the object expression first (pushes receiver on stack)
            let object_ty = self.generate_expression(*object)?;

            // Resolve underlying struct TypeId
            let struct_type_id = if let Type::Reference(boxed) = &object_ty {
                if let Type::Struct(id) = **boxed {
                    Some(id)
                } else {
                    None
                }
            } else if let Type::Struct(id) = object_ty {
                Some(id)
            } else {
                None
            };

            if struct_type_id.is_none() {
                return Err(SemanticError::TypeMismatch {
                    lhs: object_ty.to_string(),
                    rhs: "Struct instance".to_string(),
                    pos: Some(SourcePos {
                        line: field.line,
                        col: field.col,
                    }),
                });
            }

            let type_id = struct_type_id.unwrap();

            // Find the struct symbol corresponding to the TypeId
            let struct_symbol_id_opt = self
                .symbol_table
                .symbols
                .iter()
                .enumerate()
                .find(|(_, s)| match s.kind {
                    SymbolKind::Struct(id) => id == type_id,
                    _ => false,
                })
                .map(|(i, _)| i as u32);

            let struct_symbol_id = struct_symbol_id_opt.ok_or(SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: Some(SourcePos {
                    line: field.line,
                    col: field.col,
                }),
            })?;

            let struct_symbol = self.symbol_table.get_symbol(struct_symbol_id).unwrap();
            let method_symbol_id = struct_symbol.children.get(&field.name).cloned().ok_or(
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
                &format!("{}.{}", struct_symbol.name, field.name),
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
            // First try type-name constructor (e.g., Point(...))
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

                self.push_typed_argument_list(ordered_exprs, &function_udt.params, call_line, call_col)?;
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
    fn process_arguments(
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

    fn push_argument_list(&mut self, arguments: Vec<Expression>) -> SaResult<()> {
        for expr in arguments {
            self.generate_expression(expr)?;
        }

        Ok(())
    }

    fn push_typed_argument_list(
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
            let arg_ty = self.generate_expression(expr)?;

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
                    // Keep existing loose typing rules for non-ref parameters for now.
                }
            }
        }

        Ok(())
    }

    fn get_semantic_type(&mut self, parsed_type: &ParsedType) -> SaResult<Type> {
        match parsed_type {
            ParsedType::Identifier(identifier) => {
                if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
                    Ok(ty)
                } else {
                    Err(SemanticError::TypeNotFound {
                        name: identifier.name.clone(),
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })
                }
            }
            ParsedType::Reference(r) => {
                let ty = self.get_semantic_type(r)?;
                Ok(Type::Reference(Box::new(ty)))
            }
            ParsedType::Array(a) => {
                let ty = self.get_semantic_type(a)?;
                Ok(Type::Array(Box::new(ty)))
            }
            ParsedType::Generic { base, type_args } => {
                // Implement proper generic type resolution with monomorphization
                
                // First, find the base generic type
                let base_type = if let Some(ty) = self.symbol_table.find_type_in_scope(&base.name) {
                    ty
                } else {
                    return Err(SemanticError::TypeNotFound {
                        name: base.name.clone(),
                        pos: Some(SourcePos {
                            line: base.line,
                            col: base.col,
                        }),
                    });
                };
                
                // Resolve all type arguments
                let mut resolved_type_args = Vec::new();
                for arg in type_args {
                    resolved_type_args.push(self.get_semantic_type(arg)?);
                }
                
                // For now, only handle struct monomorphization
                match base_type {
                    Type::Struct(base_type_id) => {
                        self.monomorphize_struct(base_type_id, resolved_type_args, &base.name, base.line, base.col)
                    }
                    _ => {
                        // For non-struct types, just return the base type for now
                        // TODO: implement monomorphization for functions and enums
                        Ok(base_type)
                    }
                }
            }
        }
    }

    /// Monomorphize a generic struct with concrete type arguments
    fn monomorphize_struct(
        &mut self,
        base_type_id: TypeId,
        type_args: Vec<Type>,
        base_name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        // Check cache first
        let cache_key = (base_type_id, type_args.clone());
        if let Some(&cached_type_id) = self.symbol_table.monomorphization_cache.get(&cache_key) {
            return Ok(Type::Struct(cached_type_id));
        }
        
        // Get the base generic struct definition
        let base_struct = match self.symbol_table.get_udt(base_type_id) {
            UserDefinedType::Struct(s) => s.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "struct".to_string(),
                    rhs: "non-struct".to_string(),
                    pos: Some(SourcePos { line, col }),
                });
            }
        };
        
        // Validate number of type arguments matches type parameters
        if base_struct.type_parameters.len() != type_args.len() {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("{} type parameters", base_struct.type_parameters.len()),
                rhs: format!("{} type arguments", type_args.len()),
                pos: Some(SourcePos { line, col }),
            });
        }
        
        // If the struct has no type parameters, just return it as-is
        if base_struct.type_parameters.is_empty() {
            return Ok(Type::Struct(base_type_id));
        }
        
        // Get the parsed field types
        let parsed_field_types = base_struct.generic_field_types.as_ref().ok_or_else(|| {
            SemanticError::TypeMismatch {
                lhs: "generic struct".to_string(),
                rhs: "missing generic field types".to_string(),
                pos: Some(SourcePos { line, col }),
            }
        })?;
        
        // Build type parameter substitution map for parsed types
        let mut parsed_type_substitution: std::collections::HashMap<String, ParsedType> = 
            std::collections::HashMap::new();
        for (param_name, concrete_type) in base_struct.type_parameters.iter().zip(type_args.iter()) {
            // Convert the semantic Type back to ParsedType for substitution
            let parsed_type = self.type_to_parsed_type(concrete_type);
            parsed_type_substitution.insert(param_name.clone(), parsed_type);
        }
        
        // Substitute and resolve field types
        let mut monomorphized_fields: Vec<(String, Type)> = Vec::new();
        for (i, (field_name, _)) in base_struct.fields.iter().enumerate() {
            let parsed_field_type = &parsed_field_types[i];
            let substituted_parsed_type = self.substitute_parsed_type(
                parsed_field_type,
                &parsed_type_substitution
            );
            let resolved_type = self.get_semantic_type(&substituted_parsed_type)?;
            monomorphized_fields.push((field_name.clone(), resolved_type));
        }
        
        // Create monomorphized struct name
        let type_args_str = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let monomorphized_name = format!("{}<{}>", base_struct.qualified_name, type_args_str);
        
        // Create the monomorphized struct
        let monomorphized_struct = Struct {
            qualified_name: monomorphized_name,
            fields: monomorphized_fields,
            field_defaults: base_struct.field_defaults.clone(),
            attributes: base_struct.attributes.clone(),
            type_parameters: Vec::new(), // Monomorphized structs have no type parameters
            generic_field_types: None,
            monomorphization: Some((base_type_id, type_args.clone())),
        };
        
        // Add to type table
        let new_type_id = self.symbol_table.add_udt(UserDefinedType::Struct(monomorphized_struct));
        
        // Cache it
        self.symbol_table.monomorphization_cache.insert(cache_key, new_type_id);
        
        Ok(Type::Struct(new_type_id))
    }
    
    /// Monomorphize a generic function with concrete type arguments
    fn monomorphize_function(
        &mut self,
        base_type_id: TypeId,
        type_args: Vec<Type>,
        base_name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<(u32, TypeId, u32)> {  // Returns (address, type_id, symbol_id)
        // Check cache first
        let cache_key = (base_type_id, type_args.clone());
        if let Some(&cached_type_id) = self.symbol_table.monomorphization_cache.get(&cache_key) {
            // Find the address of the cached function
            let cached_func = match self.symbol_table.get_udt(cached_type_id) {
                UserDefinedType::Function(f) => f,
                _ => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "function".to_string(),
                        rhs: "non-function".to_string(),
                        pos: Some(SourcePos { line, col }),
                    });
                }
            };
            
            // Find the symbol with this function's qualified name
            for (idx, symbol) in self.symbol_table.symbols.iter().enumerate() {
                if symbol.qualified_name == cached_func.qualified_name {
                    if let SymbolKind::Function { address, type_id } = symbol.kind {
                        return Ok((address, type_id, idx as u32));
                    }
                }
            }
            
            return Err(SemanticError::TypeMismatch {
                lhs: "function symbol".to_string(),
                rhs: "not found".to_string(),
                pos: Some(SourcePos { line, col }),
            });
        }
        
        // Get the base generic function definition
        let base_func = match self.symbol_table.get_udt(base_type_id) {
            UserDefinedType::Function(f) => f.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "function".to_string(),
                    rhs: "non-function".to_string(),
                    pos: Some(SourcePos { line, col }),
                });
            }
        };
        
        // Validate number of type arguments matches type parameters
        if base_func.type_parameters.len() != type_args.len() {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("{} type parameters", base_func.type_parameters.len()),
                rhs: format!("{} type arguments", type_args.len()),
                pos: Some(SourcePos { line, col }),
            });
        }
        
        // If the function has no type parameters, just return it as-is
        if base_func.type_parameters.is_empty() {
            // Find the address and symbol_id of the base function
            for (idx, symbol) in self.symbol_table.symbols.iter().enumerate() {
                if symbol.qualified_name == base_func.qualified_name {
                    if let SymbolKind::Function { address, type_id } = symbol.kind {
                        return Ok((address, type_id, idx as u32));
                    }
                }
            }
            return Err(SemanticError::TypeMismatch {
                lhs: "function symbol".to_string(),
                rhs: "not found".to_string(),
                pos: Some(SourcePos { line, col }),
            });
        }
        
        // Get the parsed parameter types and return type
        let parsed_param_types = base_func.generic_param_types.as_ref().ok_or_else(|| {
            SemanticError::TypeMismatch {
                lhs: "generic function".to_string(),
                rhs: "missing generic parameter types".to_string(),
                pos: Some(SourcePos { line, col }),
            }
        })?;
        
        let parsed_return_type = base_func.generic_return_type.as_ref();
        
        let body = base_func.generic_body.as_ref().ok_or_else(|| {
            SemanticError::TypeMismatch {
                lhs: "generic function".to_string(),
                rhs: "missing generic body".to_string(),
                pos: Some(SourcePos { line, col }),
            }
        })?;
        
        // Build type parameter substitution map for parsed types
        let mut parsed_type_substitution: std::collections::HashMap<String, ParsedType> = 
            std::collections::HashMap::new();
        for (param_name, concrete_type) in base_func.type_parameters.iter().zip(type_args.iter()) {
            // Convert the semantic Type back to ParsedType for substitution
            let parsed_type = self.type_to_parsed_type(concrete_type);
            parsed_type_substitution.insert(param_name.clone(), parsed_type);
        }
        
        // Substitute and resolve parameter types
        let mut monomorphized_params: Vec<Type> = Vec::new();
        for parsed_param_type in parsed_param_types.iter() {
            let substituted_parsed_type = self.substitute_parsed_type(
                parsed_param_type,
                &parsed_type_substitution
            );
            let resolved_type = self.get_semantic_type(&substituted_parsed_type)?;
            monomorphized_params.push(resolved_type);
        }
        
        // Substitute and resolve return type
        let monomorphized_return_type = if let Some(parsed_ret) = parsed_return_type {
            let substituted_parsed_type = self.substitute_parsed_type(
                parsed_ret,
                &parsed_type_substitution
            );
            self.get_semantic_type(&substituted_parsed_type)?
        } else {
            Type::Primitive(PrimitiveType::Unit)
        };
        
        // Create monomorphized function name
        let type_args_str = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let monomorphized_name = format!("{}<{}>", base_func.qualified_name, type_args_str);
        
        // Create the monomorphized function metadata
        let monomorphized_func = Function {
            qualified_name: monomorphized_name.clone(),
            params: monomorphized_params.clone(),
            param_names: base_func.param_names.clone(),
            param_defaults: base_func.param_defaults.clone(),
            return_type: monomorphized_return_type,
            attributes: base_func.attributes.clone(),
            type_parameters: Vec::new(), // Monomorphized functions have no type parameters
            generic_param_types: None,
            generic_return_type: None,
            generic_body: None,
            monomorphization: Some((base_type_id, type_args.clone())),
        };
        
        // Add to type table
        let new_type_id = self.symbol_table.add_udt(UserDefinedType::Function(monomorphized_func.clone()));
        
        // Cache it
        self.symbol_table.monomorphization_cache.insert(cache_key, new_type_id);
        
        // Create symbol for the monomorphized function with placeholder address
        // We'll generate the actual bytecode later to avoid inline generation
        let symbol = Symbol::new(
            base_name.to_string(),
            monomorphized_name.clone(),
            SymbolKind::Function {
                type_id: new_type_id,
                address: 0xFFFFFFFF, // Placeholder - will be updated when bytecode is generated
            },
        );
        
        self.symbol_table.push_symbol(symbol);
        let symbol_id = (self.symbol_table.symbols.len() - 1) as u32;
        
        // Queue this monomorphization for later bytecode generation
        self.pending_monomorphizations.push((
            symbol_id,
            new_type_id,
            body.clone(),
            monomorphized_func.param_names.clone(),
            monomorphized_params.clone(),
        ));
        
        // Don't generate bytecode here - it will be generated later
        // Just return the symbol_id so the caller can emit a call instruction
        self.symbol_table.pop_symbol();
        
        Ok((0xFFFFFFFF, new_type_id, symbol_id))
    }
    
    /// Convert a semantic Type to a ParsedType (for substitution purposes)
    fn type_to_parsed_type(&self, ty: &Type) -> ParsedType {
        match ty {
            Type::Primitive(p) => {
                let name = match p {
                    PrimitiveType::Int => "int",
                    PrimitiveType::Float => "float",
                    PrimitiveType::Bool => "bool",
                    PrimitiveType::Char => "char",
                    PrimitiveType::String => "string",
                    PrimitiveType::Unit => "unit",
                };
                ParsedType::Identifier(Identifier {
                    name: name.to_string(),
                    line: SYNTHETIC_LINE,
                    col: SYNTHETIC_COL,
                })
            }
            Type::Reference(inner) => {
                ParsedType::Reference(Box::new(self.type_to_parsed_type(inner)))
            }
            Type::Array(inner) => {
                ParsedType::Array(Box::new(self.type_to_parsed_type(inner)))
            }
            Type::Struct(type_id) => {
                // Get the struct name from the symbol table
                let udt = self.symbol_table.get_udt(*type_id);
                if let UserDefinedType::Struct(s) = udt {
                    ParsedType::Identifier(Identifier {
                        name: s.qualified_name.clone(),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                } else {
                    // Fallback
                    ParsedType::Identifier(Identifier {
                        name: format!("struct_{}", type_id),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                }
            }
            _ => {
                // For other types, create a simple identifier
                ParsedType::Identifier(Identifier {
                    name: ty.to_string(),
                    line: SYNTHETIC_LINE,
                    col: SYNTHETIC_COL,
                })
            }
        }
    }
    
    /// Substitute type parameters in a ParsedType
    fn substitute_parsed_type(
        &self,
        parsed_type: &ParsedType,
        substitution: &std::collections::HashMap<String, ParsedType>,
    ) -> ParsedType {
        match parsed_type {
            ParsedType::Identifier(ident) => {
                // Check if this identifier is a type parameter
                if let Some(concrete_type) = substitution.get(&ident.name) {
                    concrete_type.clone()
                } else {
                    parsed_type.clone()
                }
            }
            ParsedType::Reference(inner) => {
                ParsedType::Reference(Box::new(self.substitute_parsed_type(inner, substitution)))
            }
            ParsedType::Array(inner) => {
                ParsedType::Array(Box::new(self.substitute_parsed_type(inner, substitution)))
            }
            ParsedType::Generic { base, type_args } => {
                // Recursively substitute in type arguments
                let substituted_args: Vec<ParsedType> = type_args
                    .iter()
                    .map(|arg| self.substitute_parsed_type(arg, substitution))
                    .collect();
                ParsedType::Generic {
                    base: base.clone(),
                    type_args: substituted_args,
                }
            }
        }
    }
}
