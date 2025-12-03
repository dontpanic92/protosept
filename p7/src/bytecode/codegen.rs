use std::error::Error;

use crate::{
    bytecode::builder::ByteCodeBuilder,
    lexer::TokenType,
    parser::{Expression, FunctionCall, FunctionDeclaration, Statement, StructInitiation},
    semantic::{
        Enum, Function, LocalSymbolScope, PrimitiveType, Struct, Symbol, SymbolKind, SymbolTable,
        Type, UserDefinedType, Variable,
    },
};

use super::Module;

#[derive(Debug, PartialEq)]
pub enum SemanticError {
    TypeNotFound {
        name: String,
        pos: Option<(usize, usize)>,
    },
    FunctionNotFound {
        name: String,
        pos: Option<(usize, usize)>,
    },
    VariableNotFound {
        name: String,
        pos: Option<(usize, usize)>,
    },
    TypeMismatch {
        lhs: String,
        rhs: String,
        pos: Option<(usize, usize)>,
    },
    VariableOutsideFunction {
        name: String,
        pos: Option<(usize, usize)>,
    },
}

impl Error for SemanticError {}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticError::TypeNotFound { name, pos } => {
                if let Some((line, col)) = pos {
                    write!(
                        f,
                        "Type not found: {} at line: {} column: {}",
                        name, line, col
                    )
                } else {
                    write!(f, "Type not found: {}", name)
                }
            }
            SemanticError::FunctionNotFound { name, pos } => {
                if let Some((line, col)) = pos {
                    write!(
                        f,
                        "Function not found: {} at line: {} column: {}",
                        name, line, col
                    )
                } else {
                    write!(f, "Function not found: {}", name)
                }
            }
            SemanticError::VariableNotFound { name, pos } => {
                if let Some((line, col)) = pos {
                    write!(
                        f,
                        "Variable not found: {} at line: {} column: {}",
                        name, line, col
                    )
                } else {
                    write!(f, "Variable not found: {}", name)
                }
            }
            SemanticError::TypeMismatch { lhs, rhs, pos } => {
                if let Some((line, col)) = pos {
                    write!(
                        f,
                        "Type mismatch: {} != {} at line: {} column: {}",
                        lhs, rhs, line, col
                    )
                } else {
                    write!(f, "Type mismatch: {} != {}", lhs, rhs)
                }
            }
            SemanticError::VariableOutsideFunction { name, pos } => {
                if let Some((line, col)) = pos {
                    write!(
                        f,
                        "Variable cannot be defined outside functions: {} at line: {} column: {}",
                        name, line, col
                    )
                } else {
                    write!(f, "Variable cannot be defined outside functions: {}", name)
                }
            }
        }
    }
}

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
                        pos: Some((identifier.line, identifier.col)),
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

                let symbol = Symbol::new(
                    name.name.clone(),
                    qualified_name,
                    SymbolKind::Enum(type_id),
                );
                
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

                let ty = Struct {
                    qualified_name: qualified_name.clone(),
                    fields: fields_with_types,
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
                unimplemented!("branching not implemented");
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
                        pos: Some((identifier.line, identifier.col)),
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
                    // Generate RHS value
                    let rhs_ty = self.generate_expression(*right)?;
    
                    // Handle LHS without generating its value (we need the target)
                    match *left {
                        Expression::Identifier(identifier) => {
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
                                    pos: Some((identifier.line, identifier.col)),
                                });
                            }
                        }
                        Expression::FieldAccess { object, field } => {
                            // Field assignment not implemented yet (struct instance field mutation).
                            return Err(SemanticError::TypeMismatch {
                                lhs: object.get_name(),
                                rhs: format!("Field assignment '{}'", field.name),
                                pos: Some((field.line, field.col)),
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
                                pos: Some((operator.line, operator.col)),
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
                        pos: Some(pos),
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
                let (object_ty, is_static_access) = if let Expression::Identifier(ref identifier) = *object {
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
                                        .unwrap() as i32;
                                    self.builder.ldi(variant_index); // Load the enum variant's index
                                    Ok(object_ty) // The type of the field access is the enum itself
                                } else {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!("Enum '{}'", enum_def.qualified_name),
                                        rhs: format!("Unknown Enum variant '{}'", field.name),
                                        pos: Some((field.line, field.col)),
                                    });
                                }
                            } else {
                                // Field access on an enum instance (e.g., my_enum_var.Variant) is not supported for simple enums.
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Enum instance '{}'", enum_def.qualified_name),
                                    rhs: format!("Field access on Enum instance via variant '{}'", field.name),
                                    pos: Some((field.line, field.col)),
                                });
                            }
                        } else {
                            unimplemented!("Internal error: Type ID resolved to non-Enum UDT");
                        }
                    }
                    Type::Struct(type_id) => {
                        // Per instructions, Struct field access is not implemented.
                        let udt = self.symbol_table.get_udt(type_id);
                        if let UserDefinedType::Struct(struct_def) = udt {
                            if is_static_access {
                                // Static field access on Structs not yet supported.
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Struct type '{}'", struct_def.qualified_name),
                                    rhs: format!("Static field access on Struct type '{}' (not supported)", field.name),
                                    pos: Some((field.line, field.col)),
                                });
                            } else {
                                // Instance field access on Structs not yet implemented, but identified correctly.
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Struct instance '{}'", struct_def.qualified_name),
                                    rhs: "Field access requested (not implemented)".to_string(),
                                    pos: Some((field.line, field.col)),
                                });
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
                            pos: Some((field.line, field.col)),
                        });
                    }
                }
            }
            Expression::StructInitiation(initiation) => self.generate_struct_initiation(initiation),
            Expression::Block(statements) => self.generate_block(statements, vec![]),
            Expression::Try {
                try_block,
                else_block,
            } => {
                let ty = self.generate_expression(*try_block)?;
                if let Some(else_block) = else_block {
                    self.generate_expression(*else_block)?;
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
        let args = declaration
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

        let return_type = if let Some(ret) = declaration.return_type {
            self.get_semantic_type(&ret)?
        } else {
            Type::Primitive(PrimitiveType::Unit)
        };

        let ty = Function {
            qualified_name: qualified_name.clone(),
            args: args.iter().map(|var| var.ty.clone()).collect(),
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

        self.local_scope = Some(LocalSymbolScope::new(args.clone()));
        let ty = self.generate_block(declaration.body, vec![])?;
        if ty != Type::Primitive(PrimitiveType::Unit) {
            self.builder.ret();
        }

        self.local_scope = None;
        self.symbol_table.pop_symbol();

        Ok(())
    }

    fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
        for arg in call.arguments {
            self.generate_expression(arg)?;
        }

        let call_name = call.name.name.clone();

        if let Some(symbol_id) = self.symbol_table.find_symbol_in_scope(&call_name) {
            let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();
            let (_, type_id) = match symbol.kind {
                SymbolKind::Function { address, type_id } => (address, type_id),
                _ => {
                    return Err(SemanticError::FunctionNotFound {
                        name: call_name.clone(),
                        pos: Some((call.name.line, call.name.col)),
                    });
                }
            };

            self.builder.call(symbol_id);

            let ty = self.symbol_table.get_udt(type_id);
            match ty {
                UserDefinedType::Function(function) => Ok(function.return_type.clone()),
                _ => panic!("Function not found"),
            }
        } else {
            Err(SemanticError::FunctionNotFound {
                name: call_name,
                pos: Some((call.name.line, call.name.col)),
            })
        }
    }

    fn generate_struct_initiation(&mut self, _initiation: StructInitiation) -> SaResult<Type> {
        unimplemented!();
        // for (_field_name, field_value) in initiation.fields {
        //     if let Some(value) = field_value {
        //         self.generate_expression(value);
        //     } else {
        //     }
        // }
    }

    fn get_semantic_type(&self, parsed_type: &crate::parser::Type) -> SaResult<Type> {
        match parsed_type {
            crate::parser::Type::Identifier(identifier) => {
                if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
                    Ok(ty)
                } else {
                    Err(SemanticError::TypeNotFound {
                        name: identifier.name.clone(),
                        pos: Some((identifier.line, identifier.col)),
                    })
                }
            }
            crate::parser::Type::Reference(r) => {
                let ty = self.get_semantic_type(r)?;
                Ok(Type::Reference(Box::new(ty)))
            }
            crate::parser::Type::Array(a) => {
                let ty = self.get_semantic_type(a)?;
                Ok(Type::Array(Box::new(ty)))
            }
        }
    }
}
