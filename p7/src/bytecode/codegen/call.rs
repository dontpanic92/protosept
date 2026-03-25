use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::ast::{Expression, FunctionCall, Identifier};
use crate::bytecode::Instruction;
use crate::errors::{SemanticError, SourcePos};
use crate::semantic::{PrimitiveType, SymbolId, SymbolKind, Type, TypeDefinition};

use super::{Generator, SaResult};

/// Type alias for call arguments
type CallArgs = Vec<(Option<Identifier>, Expression)>;

impl Generator {
    /// Main entry point for generating function calls.
    /// Dispatches to specialized handlers based on the callee expression type.
    pub(crate) fn generate_function_call(&mut self, call: FunctionCall) -> SaResult<Type> {
        let callee_expr = *call.callee;
        let arguments = call.arguments;
        let (call_line, call_col) = callee_expr.get_pos();
        let call_name = callee_expr.get_name();

        // Handle generic instantiation: Container<int>(value) or identity<int>(42)
        if let Expression::GenericInstantiation { base, type_args } = &callee_expr {
            return self.generate_generic_instantiation_call(
                base,
                type_args,
                callee_expr.clone(),
                arguments,
                call_line,
                call_col,
            );
        }

        // Handle field-call (method or static method)
        if let Expression::FieldAccess { object, field } = &callee_expr {
            return self.generate_field_access_call(
                object,
                field,
                callee_expr.clone(),
                arguments,
                call_line,
                call_col,
            );
        }

        // Non-field callee: top-level function, constructor, or intrinsic
        self.generate_simple_call(call_name, arguments, call_line, call_col)
    }

    /// Handles generic instantiation calls like `identity<int>(42)` or `Container<int>(value)`
    fn generate_generic_instantiation_call(
        &mut self,
        base: &Identifier,
        type_args: &[crate::ast::Type],
        callee_expr: Expression,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        let symbol_id = self.require_symbol_in_scope(&base.name, base.line, base.col)?;

        let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();
        let symbol_kind = symbol.kind.clone();

        match symbol_kind {
            SymbolKind::Function { func_id, .. } => {
                self.generate_generic_function_call(func_id, base, type_args, arguments)
            }
            SymbolKind::Type(type_id)
                if matches!(
                    self.symbol_table.get_type(type_id),
                    TypeDefinition::Struct(_)
                ) =>
            {
                self.generate_generic_struct_instantiation(
                    base,
                    type_args,
                    callee_expr,
                    arguments,
                    call_line,
                    call_col,
                )
            }
            SymbolKind::Type(type_id)
                if matches!(self.symbol_table.get_type(type_id), TypeDefinition::Enum(_)) =>
            {
                self.generate_generic_enum_instantiation(
                    base,
                    type_args,
                    callee_expr,
                    arguments,
                    call_line,
                    call_col,
                )
            }
            _ => Err(SemanticError::TypeMismatch {
                lhs: "function or struct".to_string(),
                rhs: format!("symbol kind: {:?}", symbol.kind),
                pos: base.pos(),
            }),
        }
    }

    /// Handles generic function calls like `identity<int>(42)`
    fn generate_generic_function_call(
        &mut self,
        func_id: u32,
        base: &Identifier,
        type_args: &[crate::ast::Type],
        arguments: CallArgs,
    ) -> SaResult<Type> {
        let resolved_type_args = self.resolve_type_args(type_args)?;

        let (_addr, mono_func_id, symbol_id) = self.monomorphize_function(
            func_id,
            resolved_type_args,
            &base.name,
            base.line,
            base.col,
        )?;

        let function_def = self.symbol_table.get_function(mono_func_id).clone();
        let ret_type = function_def.return_type.clone();

        let ordered_exprs = self.process_arguments(
            &base.name,
            base.line,
            base.col,
            arguments,
            &function_def.param_names,
            &function_def.param_defaults,
        )?;

        // Generate argument evaluation with move tracking
        for expr in ordered_exprs {
            let move_info = self.compute_move_info(&expr);
            self.generate_expression(expr)?;
            if let Some((id, is_param)) = move_info {
                if is_param { self.mark_param_moved(id); } else { self.mark_variable_moved(id); }
            }
        }

        self.builder.call(symbol_id);
        Ok(ret_type)
    }

    /// Handles generic struct instantiation like `Container<int>(value)`
    fn generate_generic_struct_instantiation(
        &mut self,
        base: &Identifier,
        type_args: &[crate::ast::Type],
        callee_expr: Expression,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        let parsed_type = crate::ast::Type::Generic {
            base: base.clone(),
            type_args: type_args.to_vec(),
        };
        let ty = self.get_semantic_type(&parsed_type)?;

        if let Type::Struct(struct_type_id) = ty {
            self.generate_struct_from_call(
                FunctionCall {
                    callee: Box::new(callee_expr),
                    arguments,
                },
                struct_type_id,
            )
        } else {
            Err(SemanticError::TypeMismatch {
                lhs: "struct".to_string(),
                rhs: ty.to_string(),
                pos: SourcePos::at(call_line, call_col),
            })
        }
    }

    /// Handles generic enum instantiation like `Option<int>.Some(42)`
    fn generate_generic_enum_instantiation(
        &mut self,
        base: &Identifier,
        type_args: &[crate::ast::Type],
        callee_expr: Expression,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        let parsed_type = crate::ast::Type::Generic {
            base: base.clone(),
            type_args: type_args.to_vec(),
        };
        let ty = self.get_semantic_type(&parsed_type)?;

        if let Type::Enum(enum_type_id) = ty {
            self.generate_enum_variant_from_call(callee_expr, arguments, enum_type_id)
        } else {
            Err(SemanticError::TypeMismatch {
                lhs: "enum".to_string(),
                rhs: ty.to_string(),
                pos: SourcePos::at(call_line, call_col),
            })
        }
    }

    /// Handles field access calls (method calls, static methods, enum variants)
    fn generate_field_access_call(
        &mut self,
        object: &Box<Expression>,
        field: &Identifier,
        callee_expr: Expression,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        // Case 1: Generic type method/variant like `Option<int>.Some(...)`
        if let Expression::GenericInstantiation { base, type_args } = object.as_ref() {
            if let Some(result) = self.try_generate_generic_type_member_call(
                base,
                type_args,
                callee_expr.clone(),
                arguments.clone(),
            )? {
                return Ok(result);
            }
        }

        // Case 2: Module member call like `module.func(...)`
        // When identifier is also a local variable, only skip module lookup if the
        // field name is a valid method on the variable's type (prefer instance methods).
        if let Expression::Identifier(ident) = object.as_ref() {
            let mut skip_module = false;
            if let Some(scope) = self.local_scope.as_ref() {
                let var_type = scope.find_variable(&ident.name)
                    .map(|id| scope.get_variable_type(id))
                    .or_else(|| scope.find_param(&ident.name).map(|id| scope.get_param_type(id)));
                if let Some(ty) = var_type {
                    // Check if the field name is a valid method on this type
                    let deref_ty = match ty {
                        Type::BoxType(inner) => *inner,
                        Type::Reference(inner) => *inner,
                        other => other,
                    };
                    if let Type::Array(_) = deref_ty {
                        // Array builtin methods
                        let array_methods = ["len", "get", "slice", "push", "pop", "clear",
                            "set", "insert", "remove", "index_of", "join",
                            "map", "filter", "reduce", "for_each", "find", "any", "all"];
                        if array_methods.contains(&field.name.as_str()) {
                            skip_module = true;
                        }
                    }
                    // For struct types, any field name is potentially a method
                    if let Type::Struct(_) = deref_ty {
                        skip_module = true;
                    }
                }
            }
            if !skip_module {
                if let Some(result) =
                    self.try_generate_module_call(ident, field, arguments.clone(), call_line, call_col)?
                {
                    return Ok(result);
                }
            }
        }

        // Case 3: Static method or enum variant like `Type.method(...)` or `Enum.Variant(...)`
        if let Expression::Identifier(ident) = object.as_ref() {
            if let Some(result) =
                self.try_generate_static_call(ident, field, callee_expr.clone(), arguments.clone())?
            {
                return Ok(result);
            }
        }

        // Case 4: Instance method call like `obj.method(...)`
        self.generate_instance_method_call(object, field, arguments, call_line, call_col)
    }

    /// Tries to generate a call on a generic type member (e.g., `Option<int>.Some(42)`)
    fn try_generate_generic_type_member_call(
        &mut self,
        base: &Identifier,
        type_args: &[crate::ast::Type],
        callee_expr: Expression,
        arguments: CallArgs,
    ) -> SaResult<Option<Type>> {
        if self.symbol_table.find_type_in_scope(&base.name).is_none() {
            return Ok(None);
        }

        let parsed_type = crate::ast::Type::Generic {
            base: base.clone(),
            type_args: type_args.to_vec(),
        };
        let concrete_ty = self.get_semantic_type(&parsed_type)?;

        if let Type::Enum(type_id) = concrete_ty {
            return Ok(Some(self.generate_enum_variant_from_call(
                callee_expr,
                arguments,
                type_id,
            )?));
        }

        // TODO: Handle static methods on generic structs if needed
        Ok(None)
    }

    /// Tries to generate a module member call (e.g., `io.println(...)`)
    /// Returns Some(return_type) if successful, None if this is not a module call
    fn try_generate_module_call(
        &mut self,
        ident: &Identifier,
        field: &Identifier,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Option<Type>> {
        // Check if the identifier refers to a module
        let Some(sym_id) = self.symbol_table.find_symbol_in_scope(&ident.name) else {
            return Ok(None);
        };
        let Some(sym) = self.symbol_table.get_symbol(sym_id) else {
            return Ok(None);
        };
        let SymbolKind::Module(module_id) = sym.kind else {
            return Ok(None);
        };
        let Some(module_info) = self.symbol_table.get_module(module_id) else {
            return Ok(None);
        };

        let module_path = module_info.path.clone();

        // Look up the member in the imported module
        let Some(member_symbol) = self.resolve_module_member(&module_path, &field.name) else {
            return Err(SemanticError::FunctionNotFound {
                name: format!("{}.{}", ident.name, field.name),
                pos: field.pos(),
            });
        };

        let member_kind = member_symbol.kind.clone();

        // Get the function ID from the symbol
        let func_id = match member_kind {
            SymbolKind::Function { func_id, .. } => func_id,
            SymbolKind::Type(_) => {
                let qualified_name = format!("{}.{}", ident.name, field.name);
                let ty = self.resolve_qualified_type_name(&qualified_name, call_line, call_col)?;
                if let Type::Struct(type_id) = ty {
                    let callee_expr = Expression::FieldAccess {
                        object: Box::new(Expression::Identifier(ident.clone())),
                        field: field.clone(),
                    };
                    let call = FunctionCall {
                        callee: Box::new(callee_expr),
                        arguments,
                    };
                    return Ok(Some(self.generate_struct_from_call(call, type_id)?));
                }

                return Err(SemanticError::TypeMismatch {
                    lhs: "Struct".to_string(),
                    rhs: ty.to_string(),
                    pos: SourcePos::at(call_line, call_col),
                });
            }
            _ => {
                return Err(SemanticError::FunctionNotFound {
                    name: format!("{}.{}", ident.name, field.name),
                    pos: field.pos(),
                });
            }
        };

        // Get the function definition from the imported module
        let imported_module = self.imported_modules.get(&module_path).ok_or_else(|| {
            SemanticError::Other(format!(
                "Module '{}' not found in imported modules",
                module_path
            ))
        })?;

        let function_def = imported_module
            .functions
            .get(func_id as usize)
            .ok_or_else(|| SemanticError::FunctionNotFound {
                name: format!("{}.{}", ident.name, field.name),
                pos: field.pos(),
            })?
            .clone();

        // Map the return type from the imported module's type table to the current
        // module's type table.  Without this, TypeIds in the return type (e.g. a
        // struct defined in the imported module) would refer to the wrong entries.
        let imported_module = self.imported_modules.get(&module_path).unwrap().clone();
        let mut type_map = std::collections::HashMap::new();
        let ret_type = self.map_type_from_module(&imported_module, &function_def.return_type, &mut type_map)?;

        // Process arguments
        let ordered_exprs = self.process_arguments(
            &format!("{}.{}", module_path, field.name),
            call_line,
            call_col,
            arguments,
            &function_def.param_names,
            &function_def.param_defaults,
        )?;

        // Generate argument evaluation with move tracking
        for expr in ordered_exprs {
            let move_info = self.compute_move_info(&expr);
            self.generate_expression(expr)?;
            if let Some((id, is_param)) = move_info {
                if is_param { self.mark_param_moved(id); } else { self.mark_variable_moved(id); }
            }
        }

        // Check if this is an intrinsic function
        if let Some(intrinsic_name) = &function_def.intrinsic_name {
            // For intrinsic functions, use InvokeHost with the intrinsic name
            let string_id = self.add_string_constant(intrinsic_name.clone());
            self.builder.call_host_function(string_id);
        } else {
            // For non-intrinsic functions, emit CallExternal instruction
            // Store module path and function name in string constants for runtime resolution
            let module_path_idx = self.add_string_constant(module_path.clone());
            let symbol_name_idx = self.add_string_constant(field.name.clone());
            self.builder.call_external(module_path_idx, symbol_name_idx);
        }

        Ok(Some(ret_type))
    }

    /// Tries to generate a static method call or enum variant construction
    fn try_generate_static_call(
        &mut self,
        ident: &Identifier,
        field: &Identifier,
        callee_expr: Expression,
        arguments: CallArgs,
    ) -> SaResult<Option<Type>> {
        let Some(ty) = self.symbol_table.find_type_in_scope(&ident.name) else {
            return Ok(None);
        };

        match ty {
            Type::Struct(_) => {
                let result = self.generate_static_struct_method_call(ident, field, arguments)?;
                Ok(Some(result))
            }
            Type::Enum(type_id) => {
                let result =
                    self.generate_enum_variant_from_call(callee_expr, arguments, type_id)?;
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    /// Generates a static method call on a struct like `Type.method(...)`
    fn generate_static_struct_method_call(
        &mut self,
        ident: &Identifier,
        field: &Identifier,
        arguments: CallArgs,
    ) -> SaResult<Type> {
        let struct_symbol_id = self.symbol_table.find_symbol_in_scope(&ident.name).ok_or(
            SemanticError::FunctionNotFound {
                name: format!("{}.{}", ident.name, field.name),
                pos: field.pos(),
            },
        )?;

        let struct_symbol = self.symbol_table.get_symbol(struct_symbol_id).unwrap();
        let method_symbol_id = struct_symbol.children.get(&field.name).cloned().ok_or(
            SemanticError::FunctionNotFound {
                name: format!("{}.{}", ident.name, field.name),
                pos: field.pos(),
            },
        )?;

        let method_symbol = self.symbol_table.get_symbol(method_symbol_id).unwrap();
        let func_id = match method_symbol.kind {
            SymbolKind::Function { func_id, .. } => func_id,
            _ => {
                return Err(SemanticError::FunctionNotFound {
                    name: format!("{}.{}", ident.name, field.name),
                    pos: field.pos(),
                });
            }
        };

        let function_def = self.symbol_table.get_function(func_id).clone();
        let ret_type = function_def.return_type.clone();

        let ordered_exprs = self.process_arguments(
            &format!("{}.{}", ident.name, field.name),
            field.line,
            field.col,
            arguments,
            &function_def.param_names,
            &function_def.param_defaults,
        )?;

        self.push_typed_argument_list(ordered_exprs, &function_def.params, field.line, field.col)?;
        self.builder.call(method_symbol_id);

        Ok(ret_type)
    }

    /// Generates an instance method call like `obj.method(...)`
    fn generate_instance_method_call(
        &mut self,
        object: &Box<Expression>,
        field: &Identifier,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        // Generate the object expression first (pushes receiver on stack)
        let object_ty = self.generate_expression(object.as_ref().clone())?;

        // Try proto dispatch for box<Proto> or ref<Proto>
        if let Some(result) =
            self.try_generate_proto_method_call(&object_ty, field, arguments.clone())?
        {
            return Ok(result);
        }

        // Handle array method calls
        if let Some(result) =
            self.try_generate_array_method_call(&object_ty, field, &arguments, call_line, call_col)?
        {
            return Ok(result);
        }

        // Handle HashMap method calls
        if let Some(result) =
            self.try_generate_hashmap_method_call(&object_ty, field, &arguments, call_line, call_col)?
        {
            return Ok(result);
        }

        // Handle primitive method calls
        if let Some(result) = self.try_generate_primitive_method_call(
            &object_ty, field, &arguments, call_line, call_col,
        )? {
            return Ok(result);
        }

        // Regular struct instance method call
        self.generate_struct_instance_method_call(&object_ty, field, arguments, call_line, call_col)
    }

    /// Tries to generate a proto method call (dynamic dispatch)
    fn try_generate_proto_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: CallArgs,
    ) -> SaResult<Option<Type>> {
        let proto_id = match object_ty {
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Proto(id) => Some(*id),
                _ => None,
            },
            Type::Reference(inner) => match inner.as_ref() {
                Type::Proto(id) => Some(*id),
                _ => None,
            },
            _ => None,
        };

        let Some(proto_id) = proto_id else {
            return Ok(None);
        };

        let result = self.generate_proto_dispatch(proto_id, field, arguments)?;
        Ok(Some(result))
    }

    /// Generates dynamic dispatch for a proto method call
    fn generate_proto_dispatch(
        &mut self,
        proto_id: u32,
        field: &Identifier,
        arguments: CallArgs,
    ) -> SaResult<Type> {
        let proto = match &self.symbol_table.types[proto_id as usize] {
            TypeDefinition::Proto(p) => p,
            _ => return Err(SemanticError::Other("Expected proto type".to_string())),
        };

        let (method_params, method_return) = proto
            .methods
            .iter()
            .find(|(name, _, _)| name == &field.name)
            .map(|(_, params, ret)| (params.clone(), ret.clone()))
            .ok_or_else(|| SemanticError::FunctionNotFound {
                name: format!("proto method {}", field.name),
                pos: field.pos(),
            })?;

        // Process arguments (skip first param which is self)
        if !method_params.is_empty() {
            for arg in arguments {
                let (_, expr) = arg;
                self.generate_expression(expr)?;
            }
        }

        // Hash the method name
        let mut hasher = DefaultHasher::new();
        field.name.hash(&mut hasher);
        let method_hash = hasher.finish() as u32;

        self.builder
            .add_instruction(Instruction::CallProtoMethod(proto_id, method_hash));

        Ok(method_return.unwrap_or(Type::Primitive(PrimitiveType::Unit)))
    }

    /// Tries to handle primitive method calls (e.g., string methods)
    fn try_generate_primitive_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: &CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Option<Type>> {
        let prim_ty = match object_ty {
            Type::Primitive(p) => Some(p),
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Primitive(p) => Some(p),
                _ => None,
            },
            Type::Reference(inner) => match inner.as_ref() {
                Type::Primitive(p) => Some(p),
                _ => None,
            },
            _ => None,
        };

        if let Some(prim_ty) = prim_ty {
            let result =
                self.handle_primitive_method_call(prim_ty, field, arguments, call_line, call_col)?;
            return Ok(Some(result));
        }

        Ok(None)
    }

    /// Tries to handle array method calls (e.g., arr.len(), arr.slice())
    fn try_generate_array_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: &CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Option<Type>> {
        // Check if the type is an array (possibly wrapped in ref or box)
        let is_array_ty = match object_ty {
            Type::Array(_) => true,
            Type::BoxType(inner) => matches!(inner.as_ref(), Type::Array(_)),
            Type::Reference(inner) => matches!(inner.as_ref(), Type::Array(_)),
            _ => false,
        };

        if !is_array_ty {
            return Ok(None);
        }

        // Delegate to helper function
        let result =
            self.handle_array_method_call(object_ty, field, arguments, call_line, call_col)?;
        Ok(Some(result))
    }

    /// Tries to generate a HashMap method call if the object is a Map type
    fn try_generate_hashmap_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: &Vec<(Option<Identifier>, Expression)>,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Option<Type>> {
        // Extract key and value types from the Map type (possibly wrapped in ref or box)
        let (key_type, val_type) = match object_ty {
            Type::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
                _ => return Ok(None),
            },
            Type::Reference(inner) => match inner.as_ref() {
                Type::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };

        let result = self.handle_hashmap_method_call(
            &key_type, &val_type, object_ty, field, arguments, call_line, call_col,
        )?;
        Ok(Some(result))
    }

    /// Generates a struct instance method call
    fn generate_struct_instance_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        // Extract the type_id for source_module lookup (works for both struct and enum)
        let type_id_opt = match object_ty {
            Type::Reference(inner) | Type::BoxType(inner) | Type::MutableReference(inner) => match inner.as_ref() {
                Type::Struct(id) | Type::Enum(id) => Some(*id),
                _ => None,
            },
            Type::Struct(id) | Type::Enum(id) => Some(*id),
            _ => None,
        };

        let type_symbol_id = type_id_opt.and_then(|id| self.symbol_table.find_symbol_for_type(id));

        // Try local method resolution first
        let local_result = type_symbol_id.and_then(|sym_id| {
            self.resolve_method(sym_id, field).ok()
        });

        if let Some((method_symbol_id, function_def)) = local_result {
            // Local method — emit Call(symbol_id)
            let param_names_full = &function_def.param_names;
            let param_defaults_full = &function_def.param_defaults;

            if param_names_full.is_empty() {
                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args expected".to_string(),
                        rhs: format!("{} provided", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                self.builder.call(method_symbol_id);
                return Ok(function_def.return_type.clone());
            }

            let type_symbol = self.symbol_table.get_symbol(type_symbol_id.unwrap()).unwrap();

            let ordered_exprs = self.process_arguments(
                &format!("{}.{}", type_symbol.name, field.name),
                field.line,
                field.col,
                arguments,
                &param_names_full[1..],
                &param_defaults_full[1..],
            )?;

            self.push_typed_argument_list(
                ordered_exprs,
                &function_def.params[1..],
                field.line,
                field.col,
            )?;
            self.builder.call(method_symbol_id);

            return Ok(function_def.return_type.clone());
        }

        // Fallback: look for the method in the source module (cross-module method call)
        let type_id = type_id_opt
            .ok_or_else(|| SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            })?;

        let (source_module_path, type_name, function_def, mapped_return_type) =
            self.resolve_external_method(type_id, field)?;

        // Process arguments using the imported function definition
        let param_names_full = &function_def.param_names;
        let param_defaults_full = &function_def.param_defaults;

        if param_names_full.is_empty() {
            if !arguments.is_empty() {
                return Err(SemanticError::TypeMismatch {
                    lhs: "0 args expected".to_string(),
                    rhs: format!("{} provided", arguments.len()),
                    pos: SourcePos::at(call_line, call_col),
                });
            }
        } else {
            let ordered_exprs = self.process_arguments(
                &format!("{}.{}", type_name, field.name),
                field.line,
                field.col,
                arguments,
                &param_names_full[1..],
                &param_defaults_full[1..],
            )?;

            // Map parameter types from source module
            let imported_module = self.imported_modules.get(&source_module_path).unwrap().clone();
            let mut type_map = std::collections::HashMap::new();
            let mapped_params: Vec<Type> = function_def.params[1..]
                .iter()
                .map(|p| self.map_type_from_module(&imported_module, p, &mut type_map))
                .collect::<SaResult<Vec<_>>>()?;

            self.push_typed_argument_list(
                ordered_exprs,
                &mapped_params,
                field.line,
                field.col,
            )?;
        }

        // Emit CallExternal for cross-module method call
        let module_path_idx = self.add_string_constant(source_module_path);
        // Use qualified method name: "TypeName.method_name"
        let method_qualified_name = format!("{}.{}", type_name, field.name);
        let symbol_name_idx = self.add_string_constant(method_qualified_name);
        self.builder.call_external(module_path_idx, symbol_name_idx);

        Ok(mapped_return_type)
    }

    /// Resolves a method from an imported module by looking up the type's source_module.
    /// Returns (module_path, type_name, function_def, mapped_return_type).
    fn resolve_external_method(
        &mut self,
        type_id: u32,
        field: &Identifier,
    ) -> SaResult<(String, String, crate::semantic::Function, Type)> {
        let type_def = self.symbol_table.types.get(type_id as usize).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        let (source_module, type_name) = match type_def {
            TypeDefinition::Struct(s) => {
                let name = s.qualified_name.rsplit('.').next().unwrap_or(&s.qualified_name).to_string();
                (s.source_module.clone(), name)
            }
            TypeDefinition::Enum(e) => {
                let name = e.qualified_name.rsplit('.').next().unwrap_or(&e.qualified_name).to_string();
                (e.source_module.clone(), name)
            }
            _ => (None, String::new()),
        };

        let module_path = source_module.ok_or_else(|| SemanticError::FunctionNotFound {
            name: field.name.clone(),
            pos: field.pos(),
        })?;

        let module = self.imported_modules.get(&module_path).cloned().ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        // Find the type symbol in the source module
        let root = module.symbols.get(0).ok_or_else(|| SemanticError::FunctionNotFound {
            name: field.name.clone(),
            pos: field.pos(),
        })?;

        let type_sym_id = root.children.get(&type_name).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        let type_sym = module.symbols.get(*type_sym_id as usize).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        // Find the method in the type's children
        let method_sym_id = type_sym.children.get(&field.name).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        let method_sym = module.symbols.get(*method_sym_id as usize).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?;

        let func_id = match method_sym.kind {
            SymbolKind::Function { func_id, .. } => func_id,
            _ => {
                return Err(SemanticError::FunctionNotFound {
                    name: field.name.clone(),
                    pos: field.pos(),
                });
            }
        };

        let function_def = module.functions.get(func_id as usize).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            }
        })?.clone();

        // Map the return type from the imported module's type table to the current module's
        let mut type_map = std::collections::HashMap::new();
        let mapped_return_type = self.map_type_from_module(&module, &function_def.return_type, &mut type_map)?;

        Ok((module_path, type_name, function_def, mapped_return_type))
    }

    /// Resolves a method symbol and its function definition (local only)
    fn resolve_method(
        &self,
        type_symbol_id: SymbolId,
        field: &Identifier,
    ) -> SaResult<(SymbolId, crate::semantic::Function)> {
        let type_symbol = self.symbol_table.get_symbol(type_symbol_id).unwrap();
        let method_symbol_id = type_symbol.children.get(&field.name).cloned().ok_or(
            SemanticError::FunctionNotFound {
                name: field.name.clone(),
                pos: field.pos(),
            },
        )?;

        let method_symbol = self.symbol_table.get_symbol(method_symbol_id).unwrap();
        let method_func_id = match method_symbol.kind {
            SymbolKind::Function { func_id, .. } => func_id,
            _ => {
                return Err(SemanticError::FunctionNotFound {
                    name: field.name.clone(),
                    pos: field.pos(),
                });
            }
        };

        let function_def = self.symbol_table.get_function(method_func_id).clone();
        Ok((method_symbol_id, function_def))
    }

    /// Handles simple calls: top-level functions, constructors, and intrinsics
    fn generate_simple_call(
        &mut self,
        call_name: String,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        // Check if the callee is a local variable of function type (closure call)
        if let Some(scope) = &self.local_scope {
            if let Some(var_id) = scope.find_variable(&call_name) {
                let var_type = scope.get_variable_type(var_id).clone();
                if let Type::Function { params: _, return_type } = var_type {
                    // Load the closure value first (it sits below the arguments on the stack)
                    self.builder.ldvar(var_id);
                    // Generate arguments
                    let arg_count = arguments.len() as u32;
                    for (_, arg_expr) in arguments {
                        self.generate_expression(arg_expr)?;
                    }
                    // Call it
                    self.builder.call_closure(arg_count);
                    return Ok(*return_type);
                }
            }
            if let Some(param_id) = scope.find_param(&call_name) {
                let param_type = scope.get_param_type(param_id).clone();
                if let Type::Function { params: _, return_type } = param_type {
                    self.builder.ldpar(param_id);
                    let arg_count = arguments.len() as u32;
                    for (_, arg_expr) in arguments {
                        self.generate_expression(arg_expr)?;
                    }
                    self.builder.call_closure(arg_count);
                    return Ok(*return_type);
                }
            }
        }

        // Handle box(expr) intrinsic
        if call_name == "box" {
            return self.generate_box_intrinsic(arguments, call_line, call_col);
        }

        // Handle Self(...) constructor
        if call_name == "Self" {
            return self.generate_self_constructor(arguments, call_line, call_col);
        }

        // Try struct constructor by type name
        if let Some(ty) = self.symbol_table.find_type_in_scope(&call_name) {
            if let Type::Struct(type_id) = ty {
                return self.generate_struct_from_call(
                    FunctionCall {
                        callee: Box::new(Expression::Identifier(Identifier {
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

        // Try function call
        self.generate_regular_function_call(call_name, arguments, call_line, call_col)
    }

    /// Generates the box(expr) intrinsic
    fn generate_box_intrinsic(
        &mut self,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        if arguments.len() != 1 {
            return Err(SemanticError::Other(format!(
                "box() requires exactly one argument, found {} at line {} column {}",
                arguments.len(),
                call_line,
                call_col
            )));
        }

        let (name_opt, expr) = &arguments[0];
        if name_opt.is_some() {
            return Err(SemanticError::Other(format!(
                "box() does not accept named arguments at line {} column {}",
                call_line, call_col
            )));
        }

        let inner_ty = self.generate_expression(expr.clone())?;
        self.builder.box_alloc();
        Ok(Type::BoxType(Box::new(inner_ty)))
    }

    /// Generates Self(...) constructor for structs
    fn generate_self_constructor(
        &mut self,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        let Some(self_type) = &self.current_self_type else {
            return Err(SemanticError::Other(format!(
                "Self can only be used inside methods at line {} column {}",
                call_line, call_col
            )));
        };

        match self_type {
            Type::Struct(type_id) => self.generate_struct_from_call(
                FunctionCall {
                    callee: Box::new(Expression::Identifier(Identifier {
                        name: "Self".to_string(),
                        line: call_line,
                        col: call_col,
                    })),
                    arguments,
                },
                *type_id,
            ),
            Type::Enum(_) => Err(SemanticError::Other(format!(
                "Self(...) constructor is not valid for enums. Use Self.VariantName(...) at line {} column {}",
                call_line, call_col
            ))),
            _ => Err(SemanticError::Other(format!(
                "Self(...) constructor is only valid for structs at line {} column {}",
                call_line, call_col
            ))),
        }
    }

    /// Generates a regular function call
    fn generate_regular_function_call(
        &mut self,
        call_name: String,
        arguments: CallArgs,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        let Some(symbol_id) = self.symbol_table.find_symbol_in_scope(&call_name) else {
            return Err(SemanticError::FunctionNotFound {
                name: call_name,
                pos: SourcePos::at(call_line, call_col),
            });
        };

        let symbol = self.symbol_table.get_symbol(symbol_id).unwrap();

        // Check if this is a struct initialization
        if let SymbolKind::Type(type_id) = symbol.kind {
            let Some(type_def) = self.symbol_table.get_type_checked(type_id) else {
                return Err(SemanticError::Other(format!(
                    "Unknown type id {} for symbol '{}' at {}:{}",
                    type_id, call_name, call_line, call_col
                )));
            };
            if matches!(type_def, TypeDefinition::Struct(_)) {
                return self.generate_struct_from_call(
                    FunctionCall {
                        callee: Box::new(Expression::Identifier(Identifier {
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
                    pos: SourcePos::at(call_line, call_col),
                });
            }
        };

        let function_def = self.symbol_table.get_function(func_id).clone();

        let ordered_exprs = self.process_arguments(
            &call_name,
            call_line,
            call_col,
            arguments,
            &function_def.param_names,
            &function_def.param_defaults,
        )?;

        self.push_typed_argument_list(ordered_exprs, &function_def.params, call_line, call_col)?;

        // Check if this is an intrinsic function
        if let Some(intrinsic_name) = &function_def.intrinsic_name {
            // For intrinsic functions, use InvokeHost instead of Call
            let string_id = self.add_string_constant(intrinsic_name.clone());
            self.builder.call_host_function(string_id);
        } else {
            // For regular functions, use Call
            self.builder.call(symbol_id);
        }

        Ok(function_def.return_type.clone())
    }
}
