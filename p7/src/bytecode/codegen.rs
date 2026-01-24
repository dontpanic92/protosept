use crate::errors::SourcePos;
use crate::{
    ast::{
        Expression, FunctionCall, FunctionDeclaration, Identifier, Statement, Type as ParsedType,
    },
    bytecode::builder::ByteCodeBuilder,
    lexer::TokenType,
    semantic::{
        Enum, Function, LocalSymbolScope, PrimitiveType, Struct, Symbol, SymbolKind, SymbolTable,
        Type, TypeId, UserDefinedType, Variable,
    },
};

use super::Module;

use crate::errors::SemanticError;

pub type SaResult<T> = Result<T, SemanticError>;

pub struct Generator {
    builder: ByteCodeBuilder,
    symbol_table: SymbolTable,
    local_scope: Option<LocalSymbolScope>,
}

impl Generator {
    pub fn new() -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: None,
        }
    }

    pub fn generate(&mut self, statements: Vec<Statement>) -> SaResult<Module> {
        for statement in statements {
            self.generate_statement(statement)?;
        }

        Ok(Module {
            instructions: self.builder.get_bytecode(),
            symbols: self.symbol_table.symbols.clone(),
            types: self.symbol_table.types.clone(),
        })
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
        for statement in statements {
            ty = self.generate_statement(statement)?;
        }

        self.local_scope.as_mut().unwrap().pop_scope();

        Ok(ty)
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
            Statement::EnumDeclaration { name, values } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                let ty = Enum {
                    qualified_name: qualified_name.clone(),
                    values: values.iter().map(|v| v.name.clone()).collect(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Enum(ty));

                let symbol =
                    Symbol::new(name.name.clone(), qualified_name, SymbolKind::Enum(type_id));

                let next_symbol_id = self.symbol_table.symbols.len() as u32;
                let current_symbol = self.symbol_table.get_current_symbol_mut().unwrap();
                current_symbol.children.insert(name.name, next_symbol_id);
                self.symbol_table.symbols.push(symbol);

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::StructDeclaration {
                name,
                fields,
                methods,
            } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                let fields_with_types = fields
                    .iter()
                    .map(|f| {
                        let field_type = self.get_semantic_type(&f.field_type).unwrap();
                        (f.name.name.clone(), field_type)
                    })
                    .collect();
                let field_defaults = fields.iter().map(|f| f.default_value.clone()).collect();

                let ty = Struct {
                    qualified_name: qualified_name.clone(),
                    fields: fields_with_types,
                    field_defaults,
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Struct(ty));

                let symbol = Symbol::new(name.name, qualified_name, SymbolKind::Struct(type_id));
                self.symbol_table.push_symbol(symbol);

                for method in methods {
                    self.process_function_declaration(method.function)?;
                }

                self.symbol_table.pop_symbol();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Branch {
                named_pattern,
                expression,
            } => {
                // Statement::Branch is used in try-else blocks for pattern matching
                // on thrown exceptions.
                
                // The exception value has already been unwrapped to a regular value
                // by the Expression::Try code generation (via UnwrapException instruction).
                // So we can just generate the pattern matching and expression code.
                
                // TODO: Implement proper pattern matching logic.
                // For now, we just generate the expression for this branch.
                // A complete implementation would need to:
                // 1. Check if the unwrapped exception value matches the pattern
                // 2. Conditionally execute this branch only if it matches
                // 3. Jump to next branch if no match
                
                // Suppress unused warning - pattern will be used in future implementation
                let _ = named_pattern;
                
                // Generate the expression that handles this branch
                let expr_type = self.generate_expression(expression)?;
                
                Ok(expr_type)
            }
            Statement::Return(expression) => {
                self.generate_expression(*expression)?;
                self.builder.ret();
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
                    TokenType::Minus => self.builder.neg(),
                    TokenType::Not => self.builder.not(),
                    _ => {
                        unimplemented!();
                    }
                }

                Ok(ty)
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
                                // Store into local variable
                                self.builder.stvar(var_id);
                                return Ok(Type::Primitive(PrimitiveType::Unit));
                            } else if let Some(param_id) = self
                                .local_scope
                                .as_mut()
                                .unwrap()
                                .find_param(&identifier.name)
                            {
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
                            
                            if let Type::Reference(struct_ref) = &object_ty
                                && let Type::Struct(type_id) = **struct_ref
                            {
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
                            } else {
                                return Err(SemanticError::TypeMismatch {
                                    lhs: object_ty.to_string(),
                                    rhs: "Struct instance".to_string(),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            }
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
                    self.builder.patch_jump_address(check_exception_placeholder, else_block_address);
                    
                    // Unwrap the exception value for the else block to use
                    self.builder.unwrap_exception();
                    
                    // Generate the else block (which may contain Statement::Branch for pattern matching)
                    self.generate_expression(*else_block)?;
                    
                    // End of exception handler
                    let end_address = self.builder.next_address();
                    self.builder.patch_jump_address(jump_over_else_placeholder, end_address);
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
        }
    }

    fn process_function_declaration(&mut self, declaration: FunctionDeclaration) -> SaResult<()> {
        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(declaration.name.name.clone());
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

        // Collect default expressions (AST expressions) for each parameter
        let param_defaults: Vec<Option<Expression>> = declaration
            .parameters
            .iter()
            .map(|param| param.default_value.clone())
            .collect();

        let return_type = if let Some(ret) = declaration.return_type {
            self.get_semantic_type(&ret)?
        } else {
            Type::Primitive(PrimitiveType::Unit)
        };

        let ty = Function {
            qualified_name: qualified_name.clone(),
            params: params.iter().map(|var| var.ty.clone()).collect(),
            param_names: params.iter().map(|var| var.name.clone()).collect(),
            param_defaults: param_defaults.clone(),
            return_type,
        };

        let type_id = self.symbol_table.add_udt(UserDefinedType::Function(ty));
        let symbol = Symbol::new(
            declaration.name.name,
            qualified_name,
            SymbolKind::Function {
                type_id,
                address: self.builder.next_address() as u32,
            },
        );

        self.symbol_table.push_symbol(symbol);

        self.local_scope = Some(LocalSymbolScope::new(params.clone()));
        let ty = self.generate_block(declaration.body, vec![])?;
        if ty != Type::Primitive(PrimitiveType::Unit) {
            self.builder.ret();
        }

        self.local_scope = None;
        self.symbol_table.pop_symbol();

        Ok(())
    }

    fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
        // Extract callee and args so we can inspect callee structure (method vs plain function)
        let callee_expr = *call.callee;
        let arguments = call.arguments;
        let (call_line, call_col) = callee_expr.get_pos();
        let call_name = callee_expr.get_name();
        
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
                        let method_symbol_id = struct_symbol
                            .children
                            .get(&field.name)
                            .cloned()
                            .ok_or(SemanticError::FunctionNotFound {
                                name: format!("{}.{}", ident.name, field.name),
                                pos: Some(SourcePos {
                                    line: field.line,
                                    col: field.col,
                                }),
                            })?;
    
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
                            UserDefinedType::Function(f) => f,
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
                        let param_defaults: Vec<Option<Expression>> = function_udt.param_defaults.clone();
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
    
                        self.push_argument_list(ordered_exprs)?;
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
            let method_symbol_id = struct_symbol
                .children
                .get(&field.name)
                .cloned()
                .ok_or(SemanticError::FunctionNotFound {
                    name: field.name.clone(),
                    pos: Some(SourcePos {
                        line: field.line,
                        col: field.col,
                    }),
                })?;
    
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
            self.push_argument_list(ordered_exprs)?;
            self.builder.call(method_symbol_id);
    
            Ok(function_udt.return_type.clone())
        } else {
            // Non-field callee: top-level function or constructor by name
            let call_name = call_name;
            // First try type-name constructor (e.g., Point(...))
            if let Some(ty) = self.symbol_table.find_type_in_scope(&call_name)
                && let Type::Struct(type_id) = ty
            {
                return self.generate_struct_from_call(crate::ast::FunctionCall {
                    callee: Box::new(Expression::Identifier(crate::ast::Identifier { name: call_name.clone(), line: call_line, col: call_col })),
                    arguments,
                }, type_id);
            }
    
            if let Some(symbol_id) = self.symbol_table.find_symbol_in_scope(&call_name) {
                let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();
    
                // Check if this is a struct initialization (struct name used as a function)
                if let SymbolKind::Struct(type_id) = symbol.kind {
                    return self.generate_struct_from_call(crate::ast::FunctionCall {
                        callee: Box::new(Expression::Identifier(crate::ast::Identifier { name: call_name.clone(), line: call_line, col: call_col })),
                        arguments,
                    }, type_id);
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
                    UserDefinedType::Function(function) => function,
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
    
                self.push_argument_list(ordered_exprs)?;
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

    fn get_semantic_type(&self, parsed_type: &ParsedType) -> SaResult<Type> {
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
        }
    }
}
