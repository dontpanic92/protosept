use crate::ast::{Expression, Identifier};
use crate::errors::SemanticError;
use crate::lexer::Token;
use crate::{
    lexer::TokenType,
    semantic::{PrimitiveType, Type, TypeDefinition},
};

use super::super::{Generator, SaResult};

impl Generator {
    pub(in crate::bytecode::codegen) fn generate_unary(
        &mut self,
        operator: Token,
        right: Expression,
    ) -> SaResult<Type> {
        let ty = self.generate_expression(right)?;
        match operator.token_type {
            TokenType::Minus => {
                self.builder.neg();
                Ok(ty)
            }
            TokenType::Not | TokenType::Exclamation => {
                self.builder.not();
                Ok(Type::Primitive(PrimitiveType::Bool))
            }
            TokenType::Multiply => self.generate_deref(ty, &operator),
            _ => unimplemented!(),
        }
    }

    pub(in crate::bytecode::codegen) fn generate_deref(
        &mut self,
        ty: Type,
        operator: &Token,
    ) -> SaResult<Type> {
        // `*r` where `r: ref<T>` yields a `T`. No runtime op yet.
        // `*b` where `b: box<T>` yields a `T` (only for primitive T).
        if let Type::Reference(inner) = ty {
            Ok(*inner)
        } else if let Type::BoxType(inner) = ty {
            match &*inner {
                Type::Primitive(_) => {
                    self.builder.box_deref();
                    Ok(*inner)
                }
                _ => Err(SemanticError::Other(format!(
                    "Cannot dereference box<{}> - only primitive types are supported at line {} column {}",
                    inner.to_string(),
                    operator.line,
                    operator.col
                ))),
            }
        } else {
            Err(self.type_mismatch_error(
                ty.to_string(),
                "ref<T> or box<T>".to_string(),
                operator.line,
                operator.col,
            ))
        }
    }

    pub(in crate::bytecode::codegen) fn generate_ref(
        &mut self,
        expr: Expression,
    ) -> SaResult<Type> {
        // Special case: ref(*b) where b is a box
        if let Expression::Unary { operator, right } = &expr
            && operator.token_type == TokenType::Multiply
        {
            let inner_ty = self.generate_expression((**right).clone())?;
            if let Type::BoxType(boxed_inner) = inner_ty {
                return Ok(Type::Reference(boxed_inner));
            }
        }

        // Check for ref(module_var) where module_var is a let mut module-level binding (§1.3.4)
        if let Expression::Identifier(ref id) = expr
            && let Some(mod_var) = self.find_module_variable(&id.name)
            && mod_var.is_mutable
        {
            return Err(SemanticError::Other(format!(
                "Cannot take ref of mutable module-level binding '{}' (let mut module-level bindings are not addressable)",
                id.name
            )));
        }

        let ty = self.generate_expression(expr)?;

        if matches!(ty, Type::Reference(_)) {
            return Err(SemanticError::Other("Cannot take ref of ref".to_string()));
        }

        Ok(Type::Reference(Box::new(ty)))
    }

    pub(in crate::bytecode::codegen) fn generate_force_unwrap(
        &mut self,
        operand: Expression,
        token: Token,
    ) -> SaResult<Type> {
        let ty = self.generate_expression(operand)?;

        // Operand must be nullable type
        let inner_ty = match ty {
            Type::Nullable(inner) => *inner,
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&ty),
                    rhs: "nullable type".to_string(),
                    pos: self.make_pos(token.line, token.col),
                });
            }
        };

        self.builder.force_unwrap();
        Ok(inner_ty)
    }

    pub(in crate::bytecode::codegen) fn generate_binary(
        &mut self,
        left: Expression,
        operator: Token,
        right: Expression,
    ) -> SaResult<Type> {
        if operator.token_type == TokenType::Assignment {
            return self.generate_assignment(left, right, &operator);
        }
        if operator.token_type == TokenType::DoubleQuestion {
            return self.generate_null_coalesce(left, right, &operator);
        }
        self.generate_binary_operation(left, operator, right)
    }

    fn generate_null_coalesce(
        &mut self,
        left: Expression,
        right: Expression,
        operator: &Token,
    ) -> SaResult<Type> {
        let lhs_ty = self.generate_expression(left)?;

        // LHS must be nullable type
        let inner_ty = match lhs_ty {
            Type::Nullable(inner) => *inner,
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&lhs_ty),
                    rhs: "nullable type".to_string(),
                    pos: self.make_pos(operator.line, operator.col),
                });
            }
        };

        let rhs_ty = self.generate_expression(right)?;

        // RHS must be compatible with inner type
        if !self.types_compatible(&rhs_ty, &inner_ty) {
            return Err(SemanticError::TypeMismatch {
                lhs: self.type_to_string(&rhs_ty),
                rhs: self.type_to_string(&inner_ty),
                pos: self.make_pos(operator.line, operator.col),
            });
        }

        self.builder.null_coalesce();
        Ok(inner_ty)
    }

    fn generate_assignment(
        &mut self,
        lhs: Expression,
        rhs: Expression,
        operator: &Token,
    ) -> SaResult<Type> {
        match lhs {
            Expression::Identifier(identifier) => {
                self.generate_assignment_to_identifier(identifier, rhs)
            }
            Expression::FieldAccess { object, field } => {
                self.generate_assignment_to_field(*object, field, rhs, operator)
            }
            Expression::ArrayIndex { array, index, pos } => {
                self.generate_assignment_to_index(*array, *index, rhs, pos)
            }
            _ => Err(SemanticError::Other(
                "Assignment to this expression is not supported".to_string(),
            )),
        }
    }

    fn generate_assignment_to_identifier(
        &mut self,
        identifier: Identifier,
        rhs: Expression,
    ) -> SaResult<Type> {
        let rhs_ty = self.generate_expression(rhs)?;

        // When inside a function/method, check local scope first
        if let Some(ref scope) = self.local_scope {
            // Try local variable first
            if let Some(var_id) = scope.find_variable(&identifier.name) {
                let lhs_ty = scope.get_variable_type(var_id);

                if matches!(lhs_ty, Type::Reference(_)) {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to read-only ref '{}'",
                        identifier.name
                    )));
                }

                if !scope.is_variable_mutable(var_id) {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to immutable variable '{}' (use 'let mut' instead of 'let')",
                        identifier.name
                    )));
                }

                if !self.types_compatible(&rhs_ty, &lhs_ty) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!(
                            "variable '{}' has type {}",
                            identifier.name,
                            lhs_ty.to_string()
                        ),
                        rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                        pos: self.make_pos(identifier.line, identifier.col),
                    });
                }

                self.builder.stvar(var_id);
                return Ok(Type::Primitive(PrimitiveType::Unit));
            }

            // Try parameter
            if let Some(param_id) = scope.find_param(&identifier.name) {
                let lhs_ty = scope.get_param_type(param_id);

                if matches!(lhs_ty, Type::Reference(_)) {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to read-only ref parameter '{}'",
                        identifier.name
                    )));
                }

                return Err(SemanticError::Other(format!(
                    "Cannot assign to immutable parameter '{}' (parameters are always immutable)",
                    identifier.name
                )));
            }
        }

        // Try module-level variable
        if let Some(mod_var) = self.find_module_variable(&identifier.name) {
            let lhs_ty = mod_var.ty.clone();
            let is_mutable = mod_var.is_mutable;
            let var_id = mod_var.var_id;

            if !is_mutable {
                return Err(SemanticError::Other(format!(
                    "Cannot assign to immutable module-level binding '{}' (use 'let mut' instead of 'let')",
                    identifier.name
                )));
            }

            if !self.types_compatible(&rhs_ty, &lhs_ty) {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!(
                        "module-level binding '{}' has type {}",
                        identifier.name,
                        lhs_ty.to_string()
                    ),
                    rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                    pos: self.make_pos(identifier.line, identifier.col),
                });
            }

            self.builder.stmodvar(var_id);
            return Ok(Type::Primitive(PrimitiveType::Unit));
        }

        Err(SemanticError::VariableNotFound {
            name: identifier.name.to_string(),
            pos: self.make_pos(identifier.line, identifier.col),
        })
    }

    fn generate_assignment_to_field(
        &mut self,
        object: Expression,
        field: Identifier,
        rhs: Expression,
        _operator: &Token,
    ) -> SaResult<Type> {
        // Check for cross-module variable assignment (module.VAR = value)
        if let Expression::Identifier(ref ident) = object
            && let Some(sym_id) = self.symbol_table.find_symbol_in_scope(&ident.name)
            && let Some(sym) = self.symbol_table.get_symbol(sym_id)
            && let crate::semantic::SymbolKind::Module(module_id) = sym.kind
            && let Some(module_info) = self.symbol_table.get_module(module_id)
        {
            let module_path = module_info.path.clone();
            if let Some(mod_var) = self.resolve_module_variable(&module_path, &field.name) {
                let raw_ty = mod_var.ty.clone();
                let is_mutable = mod_var.is_mutable;

                if !is_mutable {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to immutable module-level binding '{}.{}' (it is declared as 'pub let', not 'pub let mut')",
                        ident.name, field.name
                    )));
                }

                // Remap type IDs from the imported module's type table
                let imported_module = self.imported_modules.get(&module_path).unwrap().clone();
                let mut type_map = std::collections::HashMap::new();
                let lhs_ty = self.map_type_from_module(&imported_module, &raw_ty, &mut type_map)?;

                let rhs_ty = self.generate_expression(rhs)?;
                if !self.types_compatible(&rhs_ty, &lhs_ty) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!(
                            "module-level binding '{}.{}' has type {}",
                            ident.name,
                            field.name,
                            lhs_ty.to_string()
                        ),
                        rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                        pos: self.make_pos(field.line, field.col),
                    });
                }

                let mod_path_sid = self.add_string_constant(&module_path);
                let var_name_sid = self.add_string_constant(&field.name);
                self.builder.stextmodvar(mod_path_sid, var_name_sid);
                return Ok(Type::Primitive(PrimitiveType::Unit));
            }
            // Check if the variable exists but is private
            if let Some(imported) = self.imported_modules.get(&module_path)
                && imported
                    .module_variables
                    .iter()
                    .any(|v| v.name == field.name && !v.is_pub)
            {
                return Err(SemanticError::Other(format!(
                    "Module variable '{}' in module '{}' is private (add 'pub' to make it accessible)",
                    field.name, module_path
                )));
            }
        }

        let object_ty = self.generate_expression(object.clone())?;
        let rhs_ty = self.generate_expression(rhs)?;

        if matches!(object_ty, Type::Reference(_)) {
            return Err(SemanticError::Other(format!(
                "Cannot assign through read-only ref '{}.{}'",
                object.get_name(),
                field.name
            )));
        }

        let struct_type_id = self.extract_struct_type_id(&object_ty, &field)?;

        let udt = self.symbol_table.get_type(struct_type_id);
        if let TypeDefinition::Struct(struct_def) = udt {
            if let Some((idx, (_fname, ftype))) = struct_def
                .fields
                .iter()
                .enumerate()
                .find(|(_i, (fname, _))| fname == &field.name)
            {
                if !self.types_compatible(&rhs_ty, ftype) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("field '{}' has type {}", field.name, ftype.to_string()),
                        rhs: format!("assigned value has type {}", rhs_ty.to_string()),
                        pos: self.make_pos(field.line, field.col),
                    });
                }

                self.builder.stfield(idx as u32);
                return Ok(Type::Primitive(PrimitiveType::Unit));
            } else {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!(
                        "Struct instance '{}: {}'",
                        object.get_name(),
                        struct_def.qualified_name
                    ),
                    rhs: format!("Unknown field '.{}' on struct", field.name),
                    pos: self.make_pos(field.line, field.col),
                });
            }
        }

        unimplemented!("Internal error: Type ID resolved to non-Struct UDT");
    }

    /// Generate assignment to a boxed array element: `a[i] = expr`
    fn generate_assignment_to_index(
        &mut self,
        array: Expression,
        index: Expression,
        rhs: Expression,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let (line, col) = pos;

        // Generate array expression — must be box<array<T>>
        let array_ty = self.generate_expression(array.clone())?;
        let element_type = match &array_ty {
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Array(elem_type) => elem_type.as_ref().clone(),
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "box<array<T>>".to_string(),
                        rhs: self.type_to_string(other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            _ => {
                return Err(SemanticError::Other(format!(
                    "Cannot assign to index of non-boxed array '{}'; only box<array<T>> supports element assignment",
                    array.get_name()
                )));
            }
        };

        // Generate index expression
        let index_ty = self.generate_expression(index)?;
        if index_ty != Type::Primitive(PrimitiveType::Int) {
            return Err(SemanticError::TypeMismatch {
                lhs: "int".to_string(),
                rhs: self.type_to_string(&index_ty),
                pos: self.make_pos(line, col),
            });
        }

        // Generate RHS expression
        let rhs_ty = self.generate_expression(rhs)?;
        if !self.types_compatible(&rhs_ty, &element_type) {
            return Err(SemanticError::TypeMismatch {
                lhs: self.type_to_string(&element_type),
                rhs: self.type_to_string(&rhs_ty),
                pos: self.make_pos(line, col),
            });
        }

        // Stack is now: [box_ref, index, elem] — call array.set
        let string_id = self.add_string_constant("array.set");
        self.builder.call_host_function(string_id);

        // array.set pushes old element; discard it (assignment yields unit)
        self.builder.pop();

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(in crate::bytecode::codegen) fn generate_binary_operation(
        &mut self,
        left: Expression,
        operator: Token,
        right: Expression,
    ) -> SaResult<Type> {
        let lhs_ty = self.generate_expression(left)?;
        let rhs_ty = self.generate_expression(right)?;

        let is_bitwise = matches!(
            operator.token_type,
            TokenType::Ampersand | TokenType::Pipe | TokenType::Caret
        );

        // Bitwise operators require int operands only
        if is_bitwise {
            if lhs_ty != Type::Primitive(PrimitiveType::Int)
                || rhs_ty != Type::Primitive(PrimitiveType::Int)
            {
                return Err(self.type_mismatch_error(
                    "int".to_string(),
                    if lhs_ty != Type::Primitive(PrimitiveType::Int) {
                        lhs_ty.to_string()
                    } else {
                        rhs_ty.to_string()
                    },
                    operator.line,
                    operator.col,
                ));
            }
            self.emit_binary_instruction(&operator.token_type);
            return Ok(Type::Primitive(PrimitiveType::Int));
        }

        let is_comparison = matches!(
            operator.token_type,
            TokenType::Equals
                | TokenType::NotEquals
                | TokenType::GreaterThan
                | TokenType::GreaterThanOrEqual
                | TokenType::LessThan
                | TokenType::LessThanOrEqual
                | TokenType::And
                | TokenType::Or
        );
        let is_equality = matches!(
            operator.token_type,
            TokenType::Equals | TokenType::NotEquals
        );

        let result_ty = if lhs_ty == rhs_ty {
            lhs_ty.clone()
        } else {
            match (&lhs_ty, &rhs_ty) {
                // Allow implicit int <-> float promotion
                (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Float))
                | (Type::Primitive(PrimitiveType::Float), Type::Primitive(PrimitiveType::Int)) => {
                    Type::Primitive(PrimitiveType::Float)
                }
                // Allow null comparisons: ?T == null or null == ?T
                (Type::Nullable(_), Type::Nullable(_)) if is_equality => lhs_ty.clone(),
                // Allow string + string for concatenation
                (
                    Type::Primitive(PrimitiveType::String),
                    Type::Primitive(PrimitiveType::String),
                ) if operator.token_type == TokenType::Plus => {
                    Type::Primitive(PrimitiveType::String)
                }
                _ => {
                    return Err(self.type_mismatch_error(
                        lhs_ty.to_string(),
                        rhs_ty.to_string(),
                        operator.line,
                        operator.col,
                    ));
                }
            }
        };

        // For string + string, emit concat intrinsic instead of Add instruction
        if operator.token_type == TokenType::Plus
            && matches!(result_ty, Type::Primitive(PrimitiveType::String))
        {
            let host_fn_idx = self.add_string_constant("string.concat");
            self.builder.call_host_function(host_fn_idx);
        } else {
            self.emit_binary_instruction(&operator.token_type);
        }

        if is_comparison {
            Ok(Type::Primitive(PrimitiveType::Bool))
        } else {
            Ok(result_ty)
        }
    }

    fn emit_binary_instruction(&mut self, op: &TokenType) {
        match op {
            TokenType::Plus => self.builder.addi(),
            TokenType::Minus => self.builder.subi(),
            TokenType::Multiply => self.builder.muli(),
            TokenType::Divide => self.builder.divi(),
            TokenType::Percent => self.builder.modi(),
            TokenType::Ampersand => self.builder.bitand(),
            TokenType::Pipe => self.builder.bitor(),
            TokenType::Caret => self.builder.bitxor(),
            TokenType::And => self.builder.and(),
            TokenType::Or => self.builder.or(),
            TokenType::Equals => self.builder.eq(),
            TokenType::NotEquals => self.builder.neq(),
            TokenType::GreaterThan => self.builder.gt(),
            TokenType::GreaterThanOrEqual => self.builder.gte(),
            TokenType::LessThan => self.builder.lt(),
            TokenType::LessThanOrEqual => self.builder.lte(),
            _ => unimplemented!(),
        };
    }

    pub(in crate::bytecode::codegen) fn generate_if(
        &mut self,
        condition: Expression,
        then_branch: Expression,
        else_branch: Option<Expression>,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let condition_type = self.generate_expression(condition)?;
        self.expect_bool_type(&condition_type, pos.0, pos.1)?;

        self.builder.not();
        let jump_if_false_placeholder = self.builder.next_address();
        self.builder.jif(0);

        let then_ty = self.generate_expression(then_branch)?;

        if let Some(else_branch) = else_branch {
            let jump_to_skip_else_placeholder = self.builder.next_address();
            self.builder.jmp(0);

            let else_branch_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_if_false_placeholder, else_branch_address);

            let else_ty = self.generate_expression(else_branch)?;

            let end_of_if_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_to_skip_else_placeholder, end_of_if_address);

            // If both branches produce the same non-Unit type, this is a
            // value-producing if-expression (e.g. `let x = if c { a } else { b }`).
            if then_ty != Type::Primitive(PrimitiveType::Unit)
                && self.types_compatible(&else_ty, &then_ty)
            {
                return Ok(then_ty);
            }
        } else {
            let end_of_if_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_if_false_placeholder, end_of_if_address);
        }

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(in crate::bytecode::codegen) fn generate_field_access(
        &mut self,
        object: Expression,
        field: Identifier,
    ) -> SaResult<Type> {
        let object_name = object.get_name();

        // Check for cross-module variable access (module.VAR) before resolving as type
        if let Expression::Identifier(ref ident) = object
            && let Some(sym_id) = self.symbol_table.find_symbol_in_scope(&ident.name)
            && let Some(sym) = self.symbol_table.get_symbol(sym_id)
            && let crate::semantic::SymbolKind::Module(module_id) = sym.kind
            && let Some(module_info) = self.symbol_table.get_module(module_id)
        {
            let module_path = module_info.path.clone();
            if let Some(mod_var) = self.resolve_module_variable(&module_path, &field.name) {
                let raw_ty = mod_var.ty.clone();
                // Remap type IDs from the imported module's type table
                let imported_module = self.imported_modules.get(&module_path).unwrap().clone();
                let mut type_map = std::collections::HashMap::new();
                let ty = self.map_type_from_module(&imported_module, &raw_ty, &mut type_map)?;
                let mod_path_sid = self.add_string_constant(&module_path);
                let var_name_sid = self.add_string_constant(&field.name);
                self.builder.ldextmodvar(mod_path_sid, var_name_sid);
                return Ok(ty);
            }
            // Check if the variable exists but is private
            if let Some(imported) = self.imported_modules.get(&module_path)
                && imported
                    .module_variables
                    .iter()
                    .any(|v| v.name == field.name && !v.is_pub)
            {
                return Err(SemanticError::Other(format!(
                    "Module variable '{}' in module '{}' is private (add 'pub' to make it accessible)",
                    field.name, module_path
                )));
            }
        }

        // Resolve object type and determine if it's a static access
        let (object_ty, is_static_access) = self.resolve_field_access_object(&object)?;

        // Auto-deref references and boxes
        let object_ty = match object_ty {
            Type::Reference(inner) | Type::MutableReference(inner) => *inner,
            Type::BoxType(inner) => *inner,
            other => other,
        };

        match object_ty {
            Type::Enum(type_id) => {
                self.generate_enum_field_access(type_id, &field, is_static_access, object_ty)
            }
            Type::Struct(type_id) => {
                self.generate_struct_field_access(type_id, &field, is_static_access, &object_name)
            }
            Type::Tuple(ref element_types) => {
                // Tuple field access: t.0, t.1, etc.
                let idx: usize = field
                    .name
                    .parse()
                    .map_err(|_| SemanticError::TypeMismatch {
                        lhs: "tuple element index".to_string(),
                        rhs: format!("'{}' is not a valid tuple index", field.name),
                        pos: self.make_pos(field.line, field.col),
                    })?;
                if idx >= element_types.len() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("tuple of {} elements", element_types.len()),
                        rhs: format!("index {} out of range", idx),
                        pos: self.make_pos(field.line, field.col),
                    });
                }
                let result_type = element_types[idx].clone();
                // Push index and call tuple.index
                self.builder.ldi(idx as i64);
                let string_id = self.add_string_constant("tuple.index");
                self.builder.call_host_function(string_id);
                Ok(result_type)
            }
            _ => Err(self.type_mismatch_error(
                object_ty.to_string(),
                "Struct or Enum instance".to_string(),
                field.line,
                field.col,
            )),
        }
    }

    fn resolve_field_access_object(&mut self, object: &Expression) -> SaResult<(Type, bool)> {
        // Check for generic instantiation (e.g., Option<int>.Some)
        if let Expression::GenericInstantiation { base, type_args } = object {
            if self.symbol_table.find_type_in_scope(&base.name).is_some() {
                let parsed_type = crate::ast::Type::Generic {
                    base: base.clone(),
                    type_args: type_args.clone(),
                };
                let concrete_ty = self.get_semantic_type(&parsed_type)?;
                return Ok((concrete_ty, true));
            } else {
                return Err(SemanticError::TypeNotFound {
                    name: base.name.to_string(),
                    pos: self.make_pos(base.line, base.col),
                });
            }
        }

        // Check for type identifier (static access)
        if let Expression::Identifier(ref identifier) = *object
            && let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name)
        {
            return Ok((ty, true));
        }

        // Check for module-qualified type access (e.g., module.Type in module.Type.Variant)
        if let Expression::FieldAccess {
            object: ref inner_obj,
            field: ref inner_field,
        } = *object
            && let Expression::Identifier(ref module_ident) = **inner_obj
        {
            let qualified_name = format!("{}.{}", module_ident.name, inner_field.name);
            if let Ok(ty) = self.resolve_qualified_type_name(
                &qualified_name,
                module_ident.line,
                module_ident.col,
            ) {
                return Ok((ty, true));
            }
        }

        // Regular expression
        Ok((self.generate_expression(object.clone())?, false))
    }

    fn generate_enum_field_access(
        &mut self,
        type_id: u32,
        field: &Identifier,
        is_static_access: bool,
        object_ty: Type,
    ) -> SaResult<Type> {
        let udt = self.symbol_table.get_type(type_id);
        let TypeDefinition::Enum(enum_def) = udt else {
            unimplemented!("Internal error: Type ID resolved to non-Enum UDT");
        };

        if !is_static_access {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Enum instance '{}'", enum_def.qualified_name),
                rhs: format!("Field access on Enum instance via variant '{}'", field.name),
                pos: self.make_pos(field.line, field.col),
            });
        }

        let variant_opt = enum_def
            .variants
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == &field.name);

        if let Some((variant_index, (_, field_types))) = variant_opt {
            if field_types.is_empty() {
                self.builder.ldi(variant_index as i64);
                Ok(object_ty)
            } else {
                Err(SemanticError::TypeMismatch {
                    lhs: format!("Enum '{}'", enum_def.qualified_name),
                    rhs: format!("Payload variant '{}' requires arguments", field.name),
                    pos: self.make_pos(field.line, field.col),
                })
            }
        } else {
            Err(SemanticError::TypeMismatch {
                lhs: format!("Enum '{}'", enum_def.qualified_name),
                rhs: format!("Unknown Enum variant '{}'", field.name),
                pos: self.make_pos(field.line, field.col),
            })
        }
    }

    fn generate_struct_field_access(
        &mut self,
        type_id: u32,
        field: &Identifier,
        is_static_access: bool,
        object_name: &str,
    ) -> SaResult<Type> {
        let udt = self.symbol_table.get_type(type_id);
        let TypeDefinition::Struct(struct_def) = udt else {
            unimplemented!("Internal error: Type ID resolved to non-Struct UDT");
        };

        if is_static_access {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Struct type '{}'", struct_def.qualified_name),
                rhs: format!(
                    "Static field access on Struct type '{}' (not supported)",
                    field.name
                ),
                pos: self.make_pos(field.line, field.col),
            });
        }

        if let Some((idx, (_fname, ftype))) = struct_def
            .fields
            .iter()
            .enumerate()
            .find(|(_i, (fname, _))| fname == &field.name)
        {
            self.builder.ldfield(idx as u32);
            Ok(ftype.clone())
        } else {
            Err(SemanticError::TypeMismatch {
                lhs: format!(
                    "Struct instance '{}: {}'",
                    object_name, struct_def.qualified_name
                ),
                rhs: format!("Unknown field '.{}' on struct", field.name),
                pos: self.make_pos(field.line, field.col),
            })
        }
    }

    pub(in crate::bytecode::codegen) fn generate_try(
        &mut self,
        try_block: Expression,
        else_arms: Vec<crate::ast::MatchArm>,
    ) -> SaResult<Type> {
        let ty = self.generate_expression(try_block)?;

        if !else_arms.is_empty() {
            let check_exception_placeholder = self.builder.next_address();
            self.builder.check_exception(0);

            let jump_over_else_placeholder = self.builder.next_address();
            self.builder.jmp(0);

            let else_block_address = self.builder.next_address();
            self.builder
                .patch_jump_address(check_exception_placeholder, else_block_address);

            self.builder.unwrap_exception();

            let exception_ty = Type::Primitive(PrimitiveType::Int);
            self.generate_pattern_matching(&else_arms, exception_ty)?;

            let end_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_over_else_placeholder, end_address);
        }

        Ok(ty)
    }

    pub(in crate::bytecode::codegen) fn generate_match(
        &mut self,
        scrutinee: Expression,
        arms: Vec<crate::ast::MatchArm>,
    ) -> SaResult<Type> {
        let scrutinee_ty = self.generate_expression(scrutinee)?;
        self.generate_pattern_matching(&arms, scrutinee_ty)
    }

    pub(in crate::bytecode::codegen) fn generate_cast(
        &mut self,
        expression: Expression,
        target_type: crate::ast::Type,
    ) -> SaResult<Type> {
        let (line, col) = expression.get_pos();
        let expr_ty = self.generate_expression(expression)?;
        let target_ty = self.get_semantic_type(&target_type)?;

        match (&expr_ty, &target_ty) {
            (Type::BoxType(inner_ty), Type::BoxType(target_inner_ty)) => {
                self.generate_wrapper_cast(inner_ty, target_inner_ty, true, &target_ty, line, col)
            }
            (Type::Reference(inner_ty), Type::Reference(target_inner_ty)) => {
                self.generate_wrapper_cast(inner_ty, target_inner_ty, false, &target_ty, line, col)
            }
            _ => Err(SemanticError::Other(format!(
                "Cast from '{}' to '{}' is not supported at line {} column {}",
                self.type_to_string(&expr_ty),
                self.type_to_string(&target_ty),
                line,
                col,
            ))),
        }
    }

    fn generate_wrapper_cast(
        &mut self,
        inner_ty: &Type,
        target_inner_ty: &Type,
        is_box: bool,
        target_ty: &Type,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        let wrapper_name = if is_box { "box" } else { "ref" };

        match (inner_ty, target_inner_ty) {
            (Type::Struct(struct_id), Type::Proto(proto_id)) => {
                self.generate_wrapper_to_proto_cast(*struct_id, *proto_id, is_box, line, col)?;
                Ok(target_ty.clone())
            }
            (Type::Enum(enum_id), Type::Proto(proto_id)) => {
                self.generate_wrapper_to_proto_cast(*enum_id, *proto_id, is_box, line, col)?;
                Ok(target_ty.clone())
            }
            _ => Err(SemanticError::TypeMismatch {
                lhs: format!("{}<{}>", wrapper_name, self.type_to_string(inner_ty)),
                rhs: format!("{}<{}>", wrapper_name, self.type_to_string(target_inner_ty)),
                pos: self.make_pos(line, col),
            }),
        }
    }
}
