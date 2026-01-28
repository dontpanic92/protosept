use crate::errors::SourcePos;
use crate::{
    ast::{Expression},
    bytecode::Instruction,
    lexer::TokenType,
    semantic::{PrimitiveType, Type, UserDefinedType},
};
use crate::errors::SemanticError;

use super::codegen::{Generator, LoopContext, SaResult};

impl Generator {
    pub(super) fn generate_expression(&mut self, expression: Expression) -> SaResult<Type> {
        match expression {
            Expression::Identifier(identifier) => {
                // Handle `Self` keyword for type references in methods
                if identifier.name == "Self" {
                    if let Some(self_type) = &self.current_self_type {
                        // Self is a type reference, not a value
                        // This is used in contexts like Self(expr) for construction
                        // Return the type but don't generate any bytecode
                        return Ok(self_type.clone());
                    } else {
                        return Err(SemanticError::Other(
                            "Self can only be used inside methods".to_string()
                        ));
                    }
                }
                
                if let Some(var_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_variable(&identifier.name)
                {
                    // Check if the variable has been moved
                    if self.is_variable_moved(var_id) {
                        return Err(SemanticError::UseAfterMove {
                            name: identifier.name,
                            pos: Some(SourcePos {
                                line: identifier.line,
                                col: identifier.col,
                            }),
                        });
                    }
                    self.builder.ldvar(var_id);
                    let ty = self.local_scope.as_mut().unwrap().get_variable_type(var_id);
                    Ok(ty)
                } else if let Some(param_id) = self
                    .local_scope
                    .as_mut()
                    .unwrap()
                    .find_param(&identifier.name)
                {
                    // Check if the parameter has been moved
                    if self.is_variable_moved(param_id) {
                        return Err(SemanticError::UseAfterMove {
                            name: identifier.name,
                            pos: Some(SourcePos {
                                line: identifier.line,
                                col: identifier.col,
                            }),
                        });
                    }
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
                // Add string to the constant pool if not already present
                let string_index = if let Some(idx) = self.string_constants.iter().position(|s| s == &value) {
                    idx as u32
                } else {
                    let idx = self.string_constants.len() as u32;
                    self.string_constants.push(value.clone());
                    idx
                };
                
                // Emit instruction to load string constant
                self.builder.lds(string_index);
                Ok(Type::Primitive(PrimitiveType::String))
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
                        // `*r` where `r: ref<T>` yields a `T`. No runtime op yet.
                        // `*b` where `b: box<T>` yields a `T` (only for primitive T).
                        if let Type::Reference(inner) = ty {
                            Ok(*inner)
                        } else if let Type::BoxType(inner) = ty {
                            // Check that inner type is primitive
                            match &*inner {
                                Type::Primitive(_) => {
                                    // Generate box deref instruction
                                    self.builder.box_deref();
                                    Ok(*inner)
                                }
                                _ => {
                                    Err(SemanticError::Other(format!(
                                        "Cannot dereference box<{}> - only primitive types are supported at line {} column {}",
                                        inner.to_string(), operator.line, operator.col
                                    )))
                                }
                            }
                        } else {
                            Err(SemanticError::TypeMismatch {
                                lhs: ty.to_string(),
                                rhs: "ref <T> or box<T>".to_string(),
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
            Expression::Ref(expr) => {
                // `ref(place)` produces a `ref<T>` typed value (view).
                // The place expression must be an addressable location.
                
                // Special case: ref(*b) where b is a box
                // According to spec, ref(*b) is allowed for ANY T, including non-Copy types
                if let Expression::Unary { operator, right } = expr.as_ref() {
                    if operator.token_type == TokenType::Multiply {
                        // This is ref(*expr), check if expr is a box
                        let inner_ty = self.generate_expression((**right).clone())?;
                        if let Type::BoxType(boxed_inner) = inner_ty {
                            // ref(*b) where b: box<T> produces ref<T>
                            // We keep the box on the stack, and the type system tracks it as ref<T>
                            return Ok(Type::Reference(boxed_inner));
                        }
                    }
                }
                
                // Default case: evaluate the expression and wrap in Reference
                let ty = self.generate_expression((*expr).clone())?;
                
                // Check that we're not creating a ref of ref
                if matches!(ty, Type::Reference(_)) {
                    return Err(SemanticError::Other(format!(
                        "Cannot take ref of ref"
                    )));
                }
                
                Ok(Type::Reference(Box::new(ty)))
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
                            let rhs_ty = self.generate_expression(*right)?;

                            // Prefer local variable, fallback to parameter
                            if let Some(var_id) = self
                                .local_scope
                                .as_mut()
                                .unwrap()
                                .find_variable(&identifier.name)
                            {
                                let lhs_ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                                
                                // `ref` is read-only: disallow assignment to ref locals.
                                if matches!(lhs_ty, Type::Reference(_)) {
                                    return Err(SemanticError::Other(format!(
                                        "Cannot assign to read-only ref '{}'",
                                        identifier.name
                                    )));
                                }
                                
                                // Check type compatibility
                                if !self.types_compatible(&rhs_ty, &lhs_ty) {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!("variable '{}' has type {}", identifier.name, lhs_ty.to_string()),
                                        rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                                        pos: Some(SourcePos {
                                            line: identifier.line,
                                            col: identifier.col,
                                        }),
                                    });
                                }

                                self.builder.stvar(var_id);
                                return Ok(Type::Primitive(PrimitiveType::Unit));
                            } else if let Some(param_id) = self
                                .local_scope
                                .as_mut()
                                .unwrap()
                                .find_param(&identifier.name)
                            {
                                let lhs_ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                                
                                // `ref` is read-only: disallow assignment to ref parameters.
                                if matches!(lhs_ty, Type::Reference(_)) {
                                    return Err(SemanticError::Other(format!(
                                        "Cannot assign to read-only ref parameter '{}'",
                                        identifier.name
                                    )));
                                }
                                
                                // Check type compatibility
                                if !self.types_compatible(&rhs_ty, &lhs_ty) {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!("parameter '{}' has type {}", identifier.name, lhs_ty.to_string()),
                                        rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                                        pos: Some(SourcePos {
                                            line: identifier.line,
                                            col: identifier.col,
                                        }),
                                    });
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
                            let rhs_ty = self.generate_expression(*right.clone())?;

                            // `ref` is read-only: disallow `ref_struct.field = ...`.
                            if matches!(object_ty, Type::Reference(_)) {
                                return Err(SemanticError::Other(format!(
                                    "Cannot assign through read-only ref '{}.{}'",
                                    object.get_name(),
                                    field.name
                                )));
                            }

                            // Handle both direct struct and boxed struct
                            let struct_type_id = match &object_ty {
                                Type::Struct(type_id) => *type_id,
                                Type::BoxType(inner) => {
                                    if let Type::Struct(type_id) = **inner {
                                        type_id
                                    } else {
                                        return Err(SemanticError::TypeMismatch {
                                            lhs: object_ty.to_string(),
                                            rhs: "Struct or box<Struct>".to_string(),
                                            pos: Some(SourcePos {
                                                line: field.line,
                                                col: field.col,
                                            }),
                                        });
                                    }
                                }
                                _ => {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: object_ty.to_string(),
                                        rhs: "Struct or box<Struct>".to_string(),
                                        pos: Some(SourcePos {
                                            line: field.line,
                                            col: field.col,
                                        }),
                                    });
                                }
                            };

                            let udt = self.symbol_table.get_udt(struct_type_id);
                            if let UserDefinedType::Struct(struct_def) = udt {
                                if let Some((idx, (_fname, ftype))) = struct_def
                                    .fields
                                    .iter()
                                    .enumerate()
                                    .find(|(_i, (fname, _))| fname == &field.name)
                                {
                                    // Check type compatibility
                                    if !self.types_compatible(&rhs_ty, ftype) {
                                        return Err(SemanticError::TypeMismatch {
                                            lhs: format!("field '{}' has type {}", field.name, ftype.to_string()),
                                            rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                                            pos: Some(SourcePos {
                                                line: field.line,
                                                col: field.col,
                                            }),
                                        });
                                    }
                                    
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
                        _ => {
                            // Other lvalues (like indexing, box deref) not supported yet.
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
                
                // First, check if object is a GenericInstantiation (e.g., Option<int>.Some)
                let (object_ty, is_static_access) = if let Expression::GenericInstantiation { base, type_args } = object.as_ref() {
                    // This is a generic type access like Option<int>.Some
                    // Try to find the base type
                    if let Some(_base_ty) = self.symbol_table.find_type_in_scope(&base.name) {
                        // Resolve the generic type to its monomorphized version
                        let parsed_type = crate::ast::Type::Generic {
                            base: base.clone(),
                            type_args: type_args.clone(),
                        };
                        let concrete_ty = self.get_semantic_type(&parsed_type)?;
                        (concrete_ty, true) // It's a static access on a generic type
                    } else {
                        return Err(SemanticError::TypeNotFound {
                            name: base.name.clone(),
                            pos: Some(SourcePos {
                                line: base.line,
                                col: base.col,
                            }),
                        });
                    }
                } else {
                    // Check if object is a type identifier (for static access)
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
                    (object_ty, is_static_access)
                };

                let object_ty = match object_ty {
                    Type::Reference(inner) => *inner,
                    Type::BoxType(inner) => *inner,  // Auto-deref boxes for field access
                    other => other,
                };


                match object_ty {
                    Type::Enum(type_id) => {
                        let udt = self.symbol_table.get_udt(type_id);
                        if let UserDefinedType::Enum(enum_def) = udt {
                            if is_static_access {
                                // Find the variant by name
                                let variant_opt = enum_def.variants.iter()
                                    .enumerate()
                                    .find(|(_, (name, _))| name == &field.name);
                                
                                if let Some((variant_index, (_, field_types))) = variant_opt {
                                    // Check if this is a unit variant (no fields)
                                    if field_types.is_empty() {
                                        // Unit variant: just load the index
                                        self.builder.ldi(variant_index as i32);
                                        Ok(object_ty) // The type is the enum itself
                                    } else {
                                        // Payload variant: this should be called as a function-like expression
                                        // EnumName.Variant(args) is handled in generate_function_call
                                        // If we reach here, it means someone wrote EnumName.Variant without calling it
                                        return Err(SemanticError::TypeMismatch {
                                            lhs: format!("Enum '{}'", enum_def.qualified_name),
                                            rhs: format!("Payload variant '{}' requires arguments", field.name),
                                            pos: Some(SourcePos {
                                                line: field.line,
                                                col: field.col,
                                            }),
                                        });
                                    }
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
                    Type::TypeDecl(type_id) => {
                        let udt = self.symbol_table.get_udt(type_id);
                        if let UserDefinedType::TypeDecl(type_decl) = udt {
                            if is_static_access {
                                // Static field access on types not supported
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Type '{}'", type_decl.qualified_name),
                                    rhs: format!(
                                        "Static field access on type '{}' (not supported)",
                                        field.name
                                    ),
                                    pos: Some(SourcePos {
                                        line: field.line,
                                        col: field.col,
                                    }),
                                });
                            } else {
                                // Instance field access: only `self.0` is allowed to access representation
                                if field.name == "0" {
                                    // Access the underlying representation value
                                    if let Some(repr_type) = &type_decl.representation {
                                        // The value is already on the stack (transparent wrapper)
                                        // No bytecode needed - the type is transparent at runtime
                                        return Ok(repr_type.clone());
                                    } else {
                                        return Err(SemanticError::TypeMismatch {
                                            lhs: format!("Type '{}'", type_decl.qualified_name),
                                            rhs: "Type has no representation to access".to_string(),
                                            pos: Some(SourcePos {
                                                line: field.line,
                                                col: field.col,
                                            }),
                                        });
                                    }
                                } else {
                                    return Err(SemanticError::TypeMismatch {
                                        lhs: format!("Type instance '{}'", type_decl.qualified_name),
                                        rhs: format!("Unknown field '.{}' (only .0 is supported for types)", field.name),
                                        pos: Some(SourcePos {
                                            line: field.line,
                                            col: field.col,
                                        }),
                                    });
                                }
                            }
                        } else {
                            unimplemented!("Internal error: Type ID resolved to non-TypeDecl UDT");
                        }
                    }
                    _ => {
                        // Primitives, Arrays, References cannot be field accessed via '.' syntax
                        return Err(SemanticError::TypeMismatch {
                            lhs: object_ty.to_string(),
                            rhs: "Struct or Enum or Type type/instance".to_string(),
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
                else_arms,
            } => {
                // Generate the try block expression
                let ty = self.generate_expression(*try_block)?;

                if !else_arms.is_empty() {
                    // After try_block, check if result is an exception
                    // If exception, jump to else_arms handler
                    let check_exception_placeholder = self.builder.next_address();
                    self.builder.check_exception(0); // Placeholder address

                    // Normal path: no exception, jump over else arms
                    let jump_over_else_placeholder = self.builder.next_address();
                    self.builder.jmp(0); // Placeholder address

                    // Exception handler starts here
                    let else_block_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(check_exception_placeholder, else_block_address);

                    // Unwrap the exception value for pattern matching
                    self.builder.unwrap_exception();

                    // Use shared pattern matching logic (exception value is on stack as scrutinee)
                    // For now, use int type for exception values (enum variant index)
                    let exception_ty = Type::Primitive(PrimitiveType::Int);
                    self.generate_pattern_matching(&else_arms, exception_ty)?;

                    // End of exception handler
                    let end_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(jump_over_else_placeholder, end_address);
                }
                // If no else_arms - exception propagates automatically

                Ok(ty)
            }
            Expression::Match { scrutinee, arms } => {
                // Generate the scrutinee expression and keep it on stack
                let scrutinee_ty = self.generate_expression(*scrutinee)?;

                // Use shared pattern matching logic
                self.generate_pattern_matching(&arms, scrutinee_ty)
            }
            Expression::BlockValue(expression) => {
                let ty = self.generate_expression(*expression)?;
                Ok(ty)
            }
            Expression::Cast { expression, target_type } => {
                // Handle cast expressions: expr as box<Proto> or expr as ref<Proto>
                let (line, col) = expression.get_pos();
                let expr_ty = self.generate_expression(*expression)?;
                let target_ty = self.get_semantic_type(&target_type)?;
                
                // Support both box<T> -> box<P> and ref<T> -> ref<P> casts
                match (&expr_ty, &target_ty) {
                    (Type::BoxType(inner_ty), Type::BoxType(target_inner_ty)) => {
                        // Check if casting box<Struct/Enum> to box<Proto>
                        match (inner_ty.as_ref(), target_inner_ty.as_ref()) {
                            (Type::Struct(struct_id), Type::Proto(proto_id)) => {
                                // Verify that the struct satisfies the proto
                                let struct_def = match &self.symbol_table.types[*struct_id as usize] {
                                    UserDefinedType::Struct(s) => s,
                                    _ => return Err(SemanticError::Other("Expected struct type".to_string())),
                                };
                                
                                // Check if struct conforms to proto (either declared or structural)
                                let conforms = struct_def.conforming_to.contains(proto_id);
                                
                                if !conforms {
                                    // Check structural conformance
                                    self.check_struct_conformance(
                                        *struct_id,
                                        &[*proto_id],
                                        line,
                                        col,
                                    )?;
                                }
                                
                                // Generate BoxToProto instruction
                                self.builder.add_instruction(Instruction::BoxToProto(*struct_id, *proto_id));
                                
                                return Ok(target_ty);
                            }
                            (Type::Enum(enum_id), Type::Proto(proto_id)) => {
                                // Verify that the enum satisfies the proto
                                let enum_def = match &self.symbol_table.types[*enum_id as usize] {
                                    UserDefinedType::Enum(e) => e,
                                    _ => return Err(SemanticError::Other("Expected enum type".to_string())),
                                };
                                
                                // Check if enum conforms to proto (either declared or structural)
                                let conforms = enum_def.conforming_to.contains(proto_id);
                                
                                if !conforms {
                                    // Check structural conformance
                                    self.check_struct_conformance(
                                        *enum_id,
                                        &[*proto_id],
                                        line,
                                        col,
                                    )?;
                                }
                                
                                // Generate BoxToProto instruction
                                self.builder.add_instruction(Instruction::BoxToProto(*enum_id, *proto_id));
                                
                                return Ok(target_ty);
                            }
                            _ => {
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("box<{}>", self.type_to_string(&**inner_ty)),
                                    rhs: format!("box<{}>", self.type_to_string(&**target_inner_ty)),
                                    pos: Some(SourcePos { line, col }),
                                });
                            }
                        }
                    }
                    (Type::Reference(inner_ty), Type::Reference(target_inner_ty)) => {
                        // Check if casting ref<Struct/Enum> to ref<Proto>
                        match (inner_ty.as_ref(), target_inner_ty.as_ref()) {
                            (Type::Struct(struct_id), Type::Proto(proto_id)) => {
                                // Verify that the struct satisfies the proto
                                let struct_def = match &self.symbol_table.types[*struct_id as usize] {
                                    UserDefinedType::Struct(s) => s,
                                    _ => return Err(SemanticError::Other("Expected struct type".to_string())),
                                };
                                
                                // Check if struct conforms to proto (either declared or structural)
                                let conforms = struct_def.conforming_to.contains(proto_id);
                                
                                if !conforms {
                                    // Check structural conformance
                                    self.check_struct_conformance(
                                        *struct_id,
                                        &[*proto_id],
                                        line,
                                        col,
                                    )?;
                                }
                                
                                // Generate RefToProto instruction
                                self.builder.add_instruction(Instruction::RefToProto(*struct_id, *proto_id));
                                
                                return Ok(target_ty);
                            }
                            (Type::Enum(enum_id), Type::Proto(proto_id)) => {
                                // Verify that the enum satisfies the proto
                                let enum_def = match &self.symbol_table.types[*enum_id as usize] {
                                    UserDefinedType::Enum(e) => e,
                                    _ => return Err(SemanticError::Other("Expected enum type".to_string())),
                                };
                                
                                // Check if enum conforms to proto (either declared or structural)
                                let conforms = enum_def.conforming_to.contains(proto_id);
                                
                                if !conforms {
                                    // Check structural conformance
                                    self.check_struct_conformance(
                                        *enum_id,
                                        &[*proto_id],
                                        line,
                                        col,
                                    )?;
                                }
                                
                                // Generate RefToProto instruction
                                self.builder.add_instruction(Instruction::RefToProto(*enum_id, *proto_id));
                                
                                return Ok(target_ty);
                            }
                            _ => {
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("ref<{}>", self.type_to_string(&**inner_ty)),
                                    rhs: format!("ref<{}>", self.type_to_string(&**target_inner_ty)),
                                    pos: Some(SourcePos { line, col }),
                                });
                            }
                        }
                    }
                    _ => {
                        return Err(SemanticError::Other(format!(
                            "Cast from '{}' to '{}' is not supported at line {} column {}",
                            self.type_to_string(&expr_ty),
                            self.type_to_string(&target_ty),
                            line,
                            col,
                        )));
                    }
                }
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
            Expression::Loop { body, pos: _ } => {
                // loop { body }
                // Generates:
                //   loop_start:
                //     <body>
                //     jmp loop_start
                
                let loop_start = self.builder.next_address();
                
                // Push new loop context onto stack
                self.loop_context_stack.push(LoopContext {
                    break_patches: Vec::new(),
                    continue_target: loop_start,
                });
                
                self.generate_expression(*body)?;
                
                // Jump back to start of loop
                self.builder.jmp(loop_start);
                
                // Get the end address for break statements to jump to
                let loop_end = self.builder.next_address();
                
                // Pop loop context and patch all break statements
                if let Some(ctx) = self.loop_context_stack.pop() {
                    for break_addr in &ctx.break_patches {
                        self.builder.patch_jump_address(*break_addr, loop_end);
                    }
                }
                
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Expression::While { condition, body, pos } => {
                // while condition { body }
                // 
                // According to spec §9.5, while semantically desugars to:
                //   loop { if condition { body } else { break; } }
                //
                // However, we generate the bytecode directly for efficiency:
                //   loop_start:
                //     <condition>
                //     not
                //     jif loop_end
                //     <body>
                //     jmp loop_start
                //   loop_end:
                
                let loop_start = self.builder.next_address();
                
                // Push new loop context onto stack
                self.loop_context_stack.push(LoopContext {
                    break_patches: Vec::new(),
                    continue_target: loop_start,
                });
                
                // Evaluate condition
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
                
                // If condition is false (not true), jump to end
                self.builder.not();
                let exit_jump_placeholder = self.builder.next_address();
                self.builder.jif(0);
                
                // Generate body
                self.generate_expression(*body)?;
                
                // Jump back to start of loop
                self.builder.jmp(loop_start);
                
                // Get the end address
                let loop_end = self.builder.next_address();
                
                // Patch the exit jump
                self.builder.patch_jump_address(exit_jump_placeholder, loop_end);
                
                // Pop loop context and patch all break statements
                if let Some(ctx) = self.loop_context_stack.pop() {
                    for break_addr in &ctx.break_patches {
                        self.builder.patch_jump_address(*break_addr, loop_end);
                    }
                }
                
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Expression::Break { value, pos } => {
                // break or break expr;
                // For now, we only support break without a value (break;)
                // which exits the current loop
                
                if value.is_some() {
                    return Err(SemanticError::Other(
                        "break with value is not yet supported".to_string(),
                    ));
                }
                
                // Check if we're in a loop
                if self.loop_context_stack.is_empty() {
                    return Err(SemanticError::Other(format!(
                        "break statement outside of loop at line {} column {}",
                        pos.0, pos.1
                    )));
                }
                
                // Record this break location to patch later
                let break_jump_addr = self.builder.next_address();
                self.builder.jmp(0); // Will be patched to loop end
                
                // Add to the current loop context's break patches
                if let Some(ctx) = self.loop_context_stack.last_mut() {
                    ctx.break_patches.push(break_jump_addr);
                }
                
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Expression::Continue { pos } => {
                // continue;
                // Jumps to the start of the current loop (re-evaluate condition for while)
                
                // Check if we're in a loop and get the continue target
                let continue_target = if let Some(ctx) = self.loop_context_stack.last() {
                    ctx.continue_target
                } else {
                    return Err(SemanticError::Other(format!(
                        "continue statement outside of loop at line {} column {}",
                        pos.0, pos.1
                    )));
                };
                
                // Jump to loop start
                self.builder.jmp(continue_target);
                
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
        }
    }
}
