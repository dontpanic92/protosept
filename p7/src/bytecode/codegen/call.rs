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
            if let Some(var_id) = move_info {
                self.mark_variable_moved(var_id);
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
        if let Expression::Identifier(ident) = object.as_ref() {
            if let Some(result) =
                self.try_generate_module_call(ident, field, arguments.clone(), call_line, call_col)?
            {
                return Ok(result);
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

        // Get the function ID from the symbol
        let func_id = match &member_symbol.kind {
            SymbolKind::Function { func_id, .. } => *func_id,
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

        let ret_type = function_def.return_type.clone();

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
            if let Some(var_id) = move_info {
                self.mark_variable_moved(var_id);
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
        if let Some(result) = self.try_generate_array_method_call(
            &object_ty, field, &arguments, call_line, call_col,
        )? {
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
        let result = self.handle_array_method_call(object_ty, field, arguments, call_line, call_col)?;
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
        let type_symbol_id = match object_ty {
            Type::Reference(inner) => match inner.as_ref() {
                Type::Struct(id) => self.symbol_table.find_symbol_for_type(*id),
                _ => None,
            },
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Struct(id) => self.symbol_table.find_symbol_for_type(*id),
                _ => None,
            },
            Type::Struct(id) => self.symbol_table.find_symbol_for_type(*id),
            _ => None,
        };

        let symbol_id = type_symbol_id
            .unwrap_or_else(|| panic!("Generating method call for type failed: {:?}", object_ty));

        let (method_symbol_id, function_def) = self.resolve_method(symbol_id, field)?;

        // For instance methods the first parameter is the receiver (self) which we've already pushed.
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

        let type_symbol = self.symbol_table.get_symbol(symbol_id).unwrap();

        // Skip receiver param
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

        Ok(function_def.return_type.clone())
    }

    /// Resolves a method symbol and its function definition
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
            if matches!(
                self.symbol_table.get_type(type_id),
                TypeDefinition::Struct(_)
            ) {
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
