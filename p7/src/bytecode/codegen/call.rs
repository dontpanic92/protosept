use crate::ast::{Expression, FunctionCall};
use crate::bytecode::Instruction;
use crate::errors::{SemanticError, SourcePos};
use crate::semantic::{PrimitiveType, SymbolKind, Type, TypeDefinition};

use super::{Generator, SaResult};

impl Generator {
    pub(crate) fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
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
                SymbolKind::Function { func_id, .. } => {
                    // This is a generic function call like identity<int>(42)
                    // Resolve all type arguments
                    let mut resolved_type_args = Vec::new();
                    for arg in type_args {
                        resolved_type_args.push(self.get_semantic_type(arg)?);
                    }

                    // Monomorphize the function
                    let (_addr, mono_func_id, symbol_id) = self.monomorphize_function(
                        func_id,
                        resolved_type_args,
                        &base.name,
                        base.line,
                        base.col,
                    )?;

                    let function_def = self.symbol_table.get_function(mono_func_id).clone();

                    let param_names: Vec<String> = function_def.param_names.clone();
                    let param_defaults: Vec<Option<Expression>> =
                        function_def.param_defaults.clone();
                    let ret_type = function_def.return_type.clone();

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
                        let move_info = self.compute_move_info(&expr);

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
                SymbolKind::Type(type_id)
                    if matches!(
                        self.symbol_table.get_type(type_id),
                        TypeDefinition::Struct(_)
                    ) =>
                {
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
                SymbolKind::Type(type_id)
                    if matches!(self.symbol_table.get_type(type_id), TypeDefinition::Enum(_)) =>
                {
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
                if let Some(_base_ty) = self.symbol_table.find_type_in_scope(&base.name) {
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

            // Case 2: Module member call like `module.func(...)` (not yet supported for cross-module calls)
            if let Expression::Identifier(ident) = object.as_ref() {
                if let Some(sym_id) = self.symbol_table.find_symbol_in_scope(&ident.name) {
                    if let Some(sym) = self.symbol_table.get_symbol(sym_id) {
                        if let SymbolKind::Module(module_id) = sym.kind {
                            if let Some(module_info) = self.symbol_table.get_module(module_id) {
                                if self
                                    .resolve_module_member(&module_info.path, &field.name)
                                    .is_some()
                                {
                                    return Err(SemanticError::Other(format!(
                                        "Cross-module calls not supported yet: {}.{}",
                                        module_info.path, field.name
                                    )));
                                } else {
                                    return Err(SemanticError::FunctionNotFound {
                                        name: format!("{}.{}", ident.name, field.name),
                                        pos: Some(SourcePos {
                                            line: field.line,
                                            col: field.col,
                                        }),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Case 3: Static method call like `Type.method(...)` (object is identifier referring to a type)
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
                        let func_id = match method_symbol.kind {
                            SymbolKind::Function { func_id, .. } => func_id,
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

                        let function_def = self.symbol_table.get_function(func_id).clone();

                        let param_names: Vec<String> = function_def.param_names.clone();
                        let param_defaults: Vec<Option<Expression>> =
                            function_def.param_defaults.clone();
                        let ret_type = function_def.return_type.clone();

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
                            &function_def.params,
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
                        TypeDefinition::Proto(p) => p,
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
                        TypeDefinition::Proto(p) => p,
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
            let type_symbol_id = match &object_ty {
                Type::Reference(inner) => match inner.as_ref() {
                    Type::Struct(id) => self.symbol_table.find_symbol_for_type(*id),
                    _ => None,
                },
                Type::Struct(id) => self.symbol_table.find_symbol_for_type(*id),
                Type::Primitive(prim_ty) => {
                    let ty = self.handle_primitive_method_call(
                        prim_ty, field, &arguments, call_line, call_col,
                    );
                    return ty;
                }
                _ => None,
            };

            let symbol_id = type_symbol_id.unwrap_or_else(|| {
                panic!("Generating method call for type failed: {:?}", object_ty)
            });

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
            let method_func_id = match method_symbol.kind {
                SymbolKind::Function { func_id, .. } => func_id,
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

            let function_def = self.symbol_table.get_function(method_func_id).clone();

            // For instance methods the first parameter is the receiver (self) which we've already pushed.
            // So process remaining parameters (skip first).
            let param_names_full: Vec<String> = function_def.param_names.clone();
            let param_defaults_full: Vec<Option<Expression>> = function_def.param_defaults.clone();

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
                return Ok(function_def.return_type.clone());
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
                &function_def.params[1..],
                field.line,
                field.col,
            )?;
            self.builder.call(method_symbol_id);

            Ok(function_def.return_type.clone())
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
                        Type::Enum(_type_id) => {
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
                if let SymbolKind::Type(type_id) = symbol.kind {
                    if matches!(
                        self.symbol_table.get_type(type_id),
                        TypeDefinition::Struct(_)
                    ) {
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
                }

                let func_id = match symbol.kind {
                    SymbolKind::Function { func_id, .. } => func_id,
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

                let function_def = self.symbol_table.get_function(func_id).clone();

                let param_names: Vec<String> = function_def.param_names.clone();
                let param_defaults: Vec<Option<Expression>> = function_def.param_defaults.clone();

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
                    &function_def.params,
                    call_line,
                    call_col,
                )?;
                self.builder.call(symbol_id);

                Ok(function_def.return_type.clone())
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
}
