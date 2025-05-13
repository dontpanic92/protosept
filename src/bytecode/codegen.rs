use std::error::Error;

use crate::{
    bytecode::{builder::ByteCodeBuilder, OpCode},
    lexer::TokenType,
    parser::{Expression, FunctionCall, Statement, StructInitiation},
    semantic::{
        Enum, Function, LocalSymbolScope, PrimitiveType, Struct, Symbol, SymbolKind, SymbolTable,
        Type, UserDefinedType, Variable,
    },
};

#[derive(Debug, PartialEq)]
pub enum SemanticError {
    TypeNotFound(String),
    FunctionNotFound(String),
    VariableNotFound(String),
    TypeMismatch(String, String),
    VariableOutsideFunction(String),
}

impl Error for SemanticError {}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticError::TypeNotFound(name) => write!(f, "Type not found: {}", name),
            SemanticError::FunctionNotFound(name) => write!(f, "Function not found: {}", name),
            SemanticError::VariableNotFound(name) => write!(f, "Variable not found: {}", name),
            SemanticError::TypeMismatch(lhs, rhs) => {
                write!(f, "Type mismatch: {} != {}", lhs, rhs)
            }
            SemanticError::VariableOutsideFunction(name) => {
                write!(f, "Variable cannot be defined outside functions: {}", name)
            }
        }
    }
}

pub type SaResult<T> = Result<T, SemanticError>;

pub struct Generator {
    builder: ByteCodeBuilder,
    symbol_table: SymbolTable,
    local_scope: LocalSymbolScope,
}

impl Generator {
    pub fn new() -> Self {
        Generator {
            builder: ByteCodeBuilder::new(),
            symbol_table: SymbolTable::new(),
            local_scope: LocalSymbolScope::new(),
        }
    }

    pub fn generate(&mut self, statements: Vec<Statement>) -> SaResult<Vec<u8>> {
        for statement in statements {
            self.generate_statement(statement)?;
        }

        Ok(self.builder.get_bytecode())
    }

    fn generate_block(
        &mut self,
        statements: Vec<Statement>,
        variables: Vec<Variable>,
    ) -> SaResult<Type> {
        self.local_scope.push_scope();
        for var in variables {
            self.local_scope.add_variable(var.name, var.ty).unwrap();
        }

        let mut ty = Type::Primitive(PrimitiveType::Unit);
        for statement in statements {
            ty = self.generate_statement(statement)?;
        }

        self.local_scope.pop_scope();

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
                    .add_variable(identifier.name.clone(), ty)
                    .map_err(|_| SemanticError::VariableOutsideFunction(identifier.name))?;

                self.builder.stvar(var_id);
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Expression(expression) => self.generate_expression(expression),
            Statement::FunctionDeclaration(declaration) => {
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
                self.generate_block(declaration.body, args)?;
                self.symbol_table.pop_symbol();

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Throw(expression) => {
                self.generate_expression(expression)?;
                self.builder.add_instruction(OpCode::THROW);
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

                let symbol = Symbol::new(name.name, qualified_name, SymbolKind::Enum(type_id));
                self.symbol_table.push_symbol(symbol);
                self.symbol_table.pop_symbol();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::StructDeclaration { name, fields } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                let ty = Struct {
                    qualified_name: qualified_name.clone(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Struct(ty));

                let symbol = Symbol::new(name.name, qualified_name, SymbolKind::Enum(type_id));
                self.symbol_table.push_symbol(symbol);
                self.symbol_table.pop_symbol();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Branch {
                named_pattern,
                expression,
            } => {
                unimplemented!("branching not implemented");
            }
        }
    }

    fn generate_expression(&mut self, expression: Expression) -> SaResult<Type> {
        match expression {
            Expression::Identifier(identifier) => {
                if let Some(var_id) = self.local_scope.find_variable(&identifier.name) {
                    self.builder.ldvar(var_id);
                    let ty = self.local_scope.get_variable_type(var_id);
                    Ok(ty)
                } else {
                    Err(SemanticError::VariableNotFound(identifier.name))
                }
            }
            Expression::IntegerLiteral(value) => {
                self.builder.ldi(value as i32);
                Ok(Type::Primitive(PrimitiveType::Int))
            }
            Expression::FloatLiteral(value) => {
                self.builder.ldf(value as f32);
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
                let lhs_ty = self.generate_expression(*left)?;
                let rhs_ty = self.generate_expression(*right)?;
                if lhs_ty != rhs_ty {
                    Err(SemanticError::TypeMismatch(
                        lhs_ty.to_string(),
                        rhs_ty.to_string(),
                    ))
                } else {
                    match operator.token_type {
                        TokenType::Plus => self.builder.addi(),
                        TokenType::Minus => self.builder.subi(),
                        TokenType::Multiply => self.builder.muli(),
                        TokenType::Divide => self.builder.divi(),
                        TokenType::Assignment => unimplemented!(),
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
                    Ok(lhs_ty)
                }
            }
            Expression::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition_type = self.generate_expression(*condition)?;
                if condition_type != Type::Primitive(PrimitiveType::Bool) {
                    return Err(SemanticError::TypeMismatch(
                        condition_type.to_string(),
                        "bool".to_string(),
                    ));
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
            Expression::Return(expression) => {
                self.generate_expression(*expression)?;
                self.builder.ret();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Expression::FieldAccess { object, field } => {
                self.generate_expression(*object)?;
                unimplemented!("field access not implemented");
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

    fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
        for arg in call.arguments {
            self.generate_expression(arg)?;
        }

        if let Some(symbol) = self.symbol_table.find_symbol_in_scope(&call.name) {
            let (address, type_id) = match symbol.kind {
                SymbolKind::Function { address, type_id } => (address, type_id),
                _ => {
                    return Err(SemanticError::FunctionNotFound(call.name));
                }
            };

            self.builder.call(address);

            let ty = self.symbol_table.get_udt(type_id);
            match ty {
                UserDefinedType::Function(function) => Ok(function.return_type.clone()),
                _ => panic!("Function not found"),
            }
        } else {
            Err(SemanticError::FunctionNotFound(call.name))
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
                    Err(SemanticError::TypeNotFound(identifier.name.clone()))
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
