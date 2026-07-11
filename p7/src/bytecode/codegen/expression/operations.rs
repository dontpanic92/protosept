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
        let right = match (operator.token_type.clone(), right) {
            (TokenType::Minus, Expression::IntegerLiteral(magnitude)) => {
                let value = magnitude
                    .checked_neg()
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or_else(|| {
                        SemanticError::Other(format!(
                            "integer literal -{} exceeds the runtime int range",
                            magnitude
                        ))
                    })?;
                self.builder.ldi(value);
                return Ok(Type::Primitive(PrimitiveType::Int));
            }
            (_, right) => right,
        };

        let ty = self.generate_expression(right)?;
        match operator.token_type {
            TokenType::Minus => {
                if !matches!(ty, Type::Primitive(primitive)
                    if primitive == PrimitiveType::Int
                        || primitive == PrimitiveType::Float
                        || primitive.is_fixed_integer())
                {
                    return Err(self.type_mismatch_error(
                        ty.to_string(),
                        "numeric type".to_string(),
                        operator.line,
                        operator.col,
                    ));
                }
                if let Type::Primitive(primitive) = ty {
                    if primitive.is_unsigned_integer() {
                        return Err(self.type_mismatch_error(
                            primitive.name().to_string(),
                            "signed numeric type".to_string(),
                            operator.line,
                            operator.col,
                        ));
                    }
                    self.builder.neg();
                    if primitive.is_fixed_integer() {
                        let (min, max) = primitive.integer_bounds().unwrap();
                        self.builder.check_int_range(min, max);
                    }
                    return Ok(Type::Primitive(primitive));
                }
                self.builder.neg();
                Ok(ty)
            }
            TokenType::Not | TokenType::Exclamation => {
                self.expect_bool_type(&ty, operator.line, operator.col)?;
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
        if let Type::Reference(inner) | Type::RefMut(inner) = ty {
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

        if ty.is_ref() {
            return Err(SemanticError::Other("Cannot take ref of ref".to_string()));
        }

        Ok(Type::Reference(Box::new(ty)))
    }

    /// `refmut(place)` forms a mutable borrowed view. Permitted only when
    /// `place` is a mutable place rooted at a mutable binding (§7).
    pub(in crate::bytecode::codegen) fn generate_refmut(
        &mut self,
        expr: Expression,
    ) -> SaResult<Type> {
        // Special case: refmut(*b) where b is a box
        if let Expression::Unary { operator, right } = &expr
            && operator.token_type == TokenType::Multiply
        {
            let inner_ty = self.generate_expression((**right).clone())?;
            if let Type::BoxType(boxed_inner) = inner_ty {
                return Ok(Type::RefMut(boxed_inner));
            }
            return Err(SemanticError::Other(
                "refmut(*x) requires x to be a box<T>".to_string(),
            ));
        }

        if let Expression::Identifier(ref id) = expr
            && let Some(mod_var) = self.find_module_variable(&id.name)
            && mod_var.is_mutable
        {
            return Err(SemanticError::Other(format!(
                "Cannot take refmut of mutable module-level binding '{}' (let mut module-level bindings are not addressable)",
                id.name
            )));
        }

        if !self.is_mutable_place(&expr) {
            return Err(SemanticError::Other(format!(
                "Cannot take refmut of '{}': it is not a mutable place (the root must be a `let mut` binding, a `box<T>`, or a `refmut<T>`)",
                expr.get_name()
            )));
        }

        let ty = self.generate_expression(expr)?;

        if ty.is_ref() {
            return Err(SemanticError::Other(
                "Cannot take refmut of a ref".to_string(),
            ));
        }

        Ok(Type::RefMut(Box::new(ty)))
    }

    /// Pure (no codegen) type query for a place expression. Returns `None` for
    /// expressions that are not addressable places.
    pub(in crate::bytecode::codegen) fn infer_place_type(&self, expr: &Expression) -> Option<Type> {
        match expr {
            Expression::Identifier(id) => {
                if let Some(scope) = &self.local_scope {
                    if let Some(var_id) = scope.find_variable(&id.name) {
                        return Some(scope.get_variable_type(var_id));
                    }
                    if let Some(param_id) = scope.find_param(&id.name) {
                        return Some(scope.get_param_type(param_id));
                    }
                }
                self.find_module_variable(&id.name).map(|mv| mv.ty.clone())
            }
            Expression::FieldAccess { object, field } => {
                let oty = self.infer_place_type(object)?;
                let base = oty.referent().unwrap_or(&oty);
                let base = match base {
                    Type::BoxType(inner) => inner.as_ref(),
                    other => other,
                };
                if let Type::Struct(type_id) = base {
                    if let TypeDefinition::Struct(s) = self.symbol_table.get_type(*type_id) {
                        return s
                            .fields
                            .iter()
                            .find(|(fname, _)| fname == &field.name)
                            .map(|(_, fty)| fty.clone());
                    }
                }
                None
            }
            Expression::ArrayIndex { array, .. } => {
                let aty = self.infer_place_type(array)?;
                let base = aty.referent().unwrap_or(&aty);
                let base = match base {
                    Type::BoxType(inner) => inner.as_ref(),
                    other => other,
                };
                if let Type::Array(elem) = base {
                    return Some(elem.as_ref().clone());
                }
                None
            }
            Expression::Unary { operator, right } if operator.token_type == TokenType::Multiply => {
                let rty = self.infer_place_type(right)?;
                match rty {
                    Type::BoxType(inner) | Type::Reference(inner) | Type::RefMut(inner) => {
                        Some(*inner)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether `expr` denotes a mutable place: writable in place / borrowable as
    /// `refmut`. A place is mutable when its root is a `let mut` binding, or it
    /// passes through a `box<T>`/`refmut<T>` handle (interior mutability). A
    /// `ref<T>` handle or an immutable `let`/parameter root is NOT mutable.
    pub(in crate::bytecode::codegen) fn is_mutable_place(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(id) => {
                if let Some(scope) = &self.local_scope {
                    if let Some(var_id) = scope.find_variable(&id.name) {
                        let ty = scope.get_variable_type(var_id);
                        // A handle binding (box/refmut) is a mutable place regardless of
                        // binding mutability; a `ref` binding is not; a value binding is
                        // mutable iff declared `let mut`.
                        return match ty {
                            Type::BoxType(_) | Type::RefMut(_) => true,
                            Type::Reference(_) => false,
                            _ => scope.is_variable_mutable(var_id),
                        };
                    }
                    if let Some(param_id) = scope.find_param(&id.name) {
                        // Parameters are immutable bindings; only handle params allow
                        // mutation through them.
                        return matches!(
                            scope.get_param_type(param_id),
                            Type::BoxType(_) | Type::RefMut(_)
                        );
                    }
                }
                // Module-level bindings are not addressable for through-mutation.
                false
            }
            Expression::FieldAccess { object, .. } => match self.infer_place_type(object) {
                Some(Type::BoxType(_)) | Some(Type::RefMut(_)) => true,
                Some(Type::Reference(_)) => false,
                _ => self.is_mutable_place(object),
            },
            Expression::ArrayIndex { array, .. } => match self.infer_place_type(array) {
                Some(Type::BoxType(_)) | Some(Type::RefMut(_)) => true,
                Some(Type::Reference(_)) => false,
                _ => self.is_mutable_place(array),
            },
            Expression::Unary { operator, right } if operator.token_type == TokenType::Multiply => {
                matches!(
                    self.infer_place_type(right),
                    Some(Type::BoxType(_)) | Some(Type::RefMut(_))
                )
            }
            _ => false,
        }
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

        let rhs_ty = self.generate_expression_with_expected_type(right, Some(&inner_ty))?;

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
        // When inside a function/method, check local scope first
        if let Some(ref scope) = self.local_scope {
            // Try local variable first
            if let Some(var_id) = scope.find_variable(&identifier.name) {
                let lhs_ty = scope.get_variable_type(var_id);

                if !scope.is_variable_mutable(var_id) {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to immutable variable '{}' (use 'let mut' instead of 'let')",
                        identifier.name
                    )));
                }

                let rhs_ty = self.generate_expression_with_expected_type(rhs, Some(&lhs_ty))?;
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
                let _lhs_ty = scope.get_param_type(param_id);

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

            let rhs_ty = self.generate_expression_with_expected_type(rhs, Some(&lhs_ty))?;
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

                let rhs_ty = self.generate_expression_with_expected_type(rhs, Some(&lhs_ty))?;
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

        // Interior mutation requires a mutable place: a `box`/`refmut` handle, or a
        // value struct rooted at a `let mut` binding. `ref<T>` and immutable `let`
        // roots are read-only.
        let writable = match &object_ty {
            Type::BoxType(_) | Type::RefMut(_) => true,
            Type::Reference(_) => false,
            _ => self.is_mutable_place(&object),
        };
        if !writable {
            return Err(SemanticError::Other(format!(
                "Cannot assign to field '{}' of '{}': it is not a mutable place (the base must be a `let mut` binding, a `box<T>`, or a `refmut<T>`; a `ref<T>` is read-only)",
                field.name,
                object.get_name()
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
                self.ensure_struct_field_visible(
                    struct_def,
                    idx,
                    &field.name,
                    field.line,
                    field.col,
                )?;
                let field_type = ftype.clone();
                let rhs_ty = self.generate_expression_with_expected_type(rhs, Some(&field_type))?;
                if !self.types_compatible(&rhs_ty, &field_type) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("field '{}' has type {}", field.name, field_type.to_string()),
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

        // Resolve the local slot of the base (if it is a bare local identifier),
        // needed for store-back when mutating a value (non-boxed) array in place.
        let base_var_id = match &array {
            Expression::Identifier(id) => self
                .local_scope
                .as_ref()
                .and_then(|scope| scope.find_variable(&id.name)),
            _ => None,
        };

        // Generate the array base. For `box<array<T>>` this pushes a box ref
        // (mutated in place); for a value `array<T>` it pushes the array value
        // (mutated via copy-on-write and stored back to its slot).
        let array_ty = self.generate_expression(array.clone())?;

        enum IndexTarget {
            Boxed,
            ValueLocal(u32),
        }

        let (element_type, target) = match &array_ty {
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Array(elem_type) => (elem_type.as_ref().clone(), IndexTarget::Boxed),
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "box<array<T>>".to_string(),
                        rhs: self.type_to_string(other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            Type::Array(elem_type) => {
                // Value array: element assignment is allowed when the base is a
                // mutable place. Store-back currently supports a bare `let mut`
                // local array; nested/handle bases should use `box<array<T>>`.
                if !self.is_mutable_place(&array) {
                    return Err(SemanticError::Other(format!(
                        "Cannot assign to index of '{}': it is not a mutable place (use a `let mut` array, or a `box<array<T>>`)",
                        array.get_name()
                    )));
                }
                let Some(var_id) = base_var_id else {
                    return Err(SemanticError::Other(format!(
                        "Element assignment to the value array '{}' is only supported when it is a `let mut` local; use `box<array<T>>` for nested or handle-rooted arrays",
                        array.get_name()
                    )));
                };
                (elem_type.as_ref().clone(), IndexTarget::ValueLocal(var_id))
            }
            _ => {
                return Err(SemanticError::Other(format!(
                    "Cannot assign to index of '{}': expected an array (`box<array<T>>` or a `let mut` array)",
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
        let rhs_ty = self.generate_expression_with_expected_type(rhs, Some(&element_type))?;
        if !self.types_compatible(&rhs_ty, &element_type) {
            return Err(SemanticError::TypeMismatch {
                lhs: self.type_to_string(&element_type),
                rhs: self.type_to_string(&rhs_ty),
                pos: self.make_pos(line, col),
            });
        }

        match target {
            IndexTarget::Boxed => {
                // Stack: [box_ref, index, elem] — array.set pushes old element.
                let string_id = self.add_string_constant("array.set");
                self.builder.call_host_function(string_id);
                self.builder.pop();
            }
            IndexTarget::ValueLocal(var_id) => {
                // Stack: [array, index, elem] — set_in_place pushes the modified
                // array; store it back into the local slot.
                let string_id = self.add_string_constant("array.set_in_place");
                self.builder.call_host_function(string_id);
                self.builder.stvar(var_id);
            }
        }

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(in crate::bytecode::codegen) fn generate_binary_operation(
        &mut self,
        left: Expression,
        operator: Token,
        right: Expression,
    ) -> SaResult<Type> {
        let is_comparison = matches!(
            operator.token_type,
            TokenType::Equals
                | TokenType::NotEquals
                | TokenType::GreaterThan
                | TokenType::GreaterThanOrEqual
                | TokenType::LessThan
                | TokenType::LessThanOrEqual
        );
        let is_equality = matches!(
            operator.token_type,
            TokenType::Equals | TokenType::NotEquals
        );

        if is_equality
            && (matches!(left, Expression::NullLiteral) || matches!(right, Expression::NullLiteral))
        {
            let left_is_null = matches!(left, Expression::NullLiteral);
            let right_is_null = matches!(right, Expression::NullLiteral);
            if left_is_null && right_is_null {
                return Err(SemanticError::Other(format!(
                    "Cannot compare two bare null literals at line {} column {}",
                    operator.line, operator.col
                )));
            }

            let non_null_ty = if left_is_null {
                self.builder.ldnull();
                self.generate_expression(right)?
            } else {
                let ty = self.generate_expression(left)?;
                self.builder.ldnull();
                ty
            };

            if !matches!(non_null_ty, Type::Nullable(_)) {
                return Err(self.type_mismatch_error(
                    non_null_ty.to_string(),
                    "nullable type".to_string(),
                    operator.line,
                    operator.col,
                ));
            }

            self.emit_binary_instruction(&operator.token_type);
            return Ok(Type::Primitive(PrimitiveType::Bool));
        }

        let lhs_ty = self.generate_expression(left)?;
        let rhs_ty = self.generate_expression(right)?;

        let is_bitwise = matches!(
            operator.token_type,
            TokenType::Ampersand | TokenType::Pipe | TokenType::Caret
        );

        // Integer bitwise operators are intrinsic. User-defined values dispatch
        // through the corresponding builtin operator protocol.
        if is_bitwise {
            if lhs_ty == rhs_ty {
                if let Type::Primitive(primitive) = lhs_ty
                    && primitive.is_integer()
                {
                    self.emit_binary_instruction(&operator.token_type);
                    return Ok(Type::Primitive(primitive));
                }
                if matches!(lhs_ty, Type::Struct(_) | Type::Enum(_)) {
                    return self.generate_bitwise_protocol_call(
                        &lhs_ty,
                        &operator.token_type,
                        operator.line,
                        operator.col,
                    );
                }
            }
            return Err(self.type_mismatch_error(
                lhs_ty.to_string(),
                rhs_ty.to_string(),
                operator.line,
                operator.col,
            ));
        }
        let is_logical = matches!(operator.token_type, TokenType::And | TokenType::Or);
        let is_arithmetic = matches!(
            operator.token_type,
            TokenType::Plus
                | TokenType::Minus
                | TokenType::Multiply
                | TokenType::Divide
                | TokenType::Percent
        );

        let numeric_result = |lhs: &Type, rhs: &Type| match (lhs, rhs) {
            (Type::Primitive(lhs), Type::Primitive(rhs))
                if lhs == rhs && lhs.is_fixed_integer() =>
            {
                Some(Type::Primitive(*lhs))
            }
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Int)) => {
                Some(Type::Primitive(PrimitiveType::Int))
            }
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Float))
            | (Type::Primitive(PrimitiveType::Float), Type::Primitive(PrimitiveType::Int))
            | (Type::Primitive(PrimitiveType::Float), Type::Primitive(PrimitiveType::Float)) => {
                Some(Type::Primitive(PrimitiveType::Float))
            }
            _ => None,
        };

        let nullable_equality_compatible = |lhs: &Type, rhs: &Type| match (lhs, rhs) {
            (Type::Nullable(lhs_inner), Type::Nullable(rhs_inner)) => {
                matches!(lhs_inner.as_ref(), Type::Primitive(PrimitiveType::Unit))
                    || matches!(rhs_inner.as_ref(), Type::Primitive(PrimitiveType::Unit))
                    || self.types_compatible(lhs_inner, rhs_inner)
                    || self.types_compatible(rhs_inner, lhs_inner)
            }
            _ => false,
        };

        let result_ty = if is_logical {
            self.expect_bool_type(&lhs_ty, operator.line, operator.col)?;
            self.expect_bool_type(&rhs_ty, operator.line, operator.col)?;
            Type::Primitive(PrimitiveType::Bool)
        } else if is_arithmetic {
            if operator.token_type == TokenType::Plus
                && lhs_ty == Type::Primitive(PrimitiveType::String)
                && rhs_ty == Type::Primitive(PrimitiveType::String)
            {
                Type::Primitive(PrimitiveType::String)
            } else if operator.token_type == TokenType::Percent {
                if lhs_ty != Type::Primitive(PrimitiveType::Int)
                    || rhs_ty != Type::Primitive(PrimitiveType::Int)
                {
                    return Err(self.type_mismatch_error(
                        lhs_ty.to_string(),
                        rhs_ty.to_string(),
                        operator.line,
                        operator.col,
                    ));
                }
                Type::Primitive(PrimitiveType::Int)
            } else if let Some(result) = numeric_result(&lhs_ty, &rhs_ty) {
                if let Type::Primitive(primitive) = result
                    && primitive.is_fixed_integer()
                {
                    self.emit_binary_instruction(&operator.token_type);
                    let (min, max) = primitive.integer_bounds().unwrap();
                    self.builder.check_int_range(min, max);
                    return Ok(Type::Primitive(primitive));
                }
                result
            } else {
                return Err(self.type_mismatch_error(
                    lhs_ty.to_string(),
                    rhs_ty.to_string(),
                    operator.line,
                    operator.col,
                ));
            }
        } else if is_equality {
            if lhs_ty == rhs_ty
                || self.types_compatible(&lhs_ty, &rhs_ty)
                || self.types_compatible(&rhs_ty, &lhs_ty)
                || nullable_equality_compatible(&lhs_ty, &rhs_ty)
            {
                Type::Primitive(PrimitiveType::Bool)
            } else {
                return Err(self.type_mismatch_error(
                    lhs_ty.to_string(),
                    rhs_ty.to_string(),
                    operator.line,
                    operator.col,
                ));
            }
        } else if is_comparison {
            if numeric_result(&lhs_ty, &rhs_ty).is_none() {
                return Err(self.type_mismatch_error(
                    lhs_ty.to_string(),
                    rhs_ty.to_string(),
                    operator.line,
                    operator.col,
                ));
            }
            Type::Primitive(PrimitiveType::Bool)
        } else if lhs_ty == rhs_ty {
            lhs_ty.clone()
        } else {
            return Err(self.type_mismatch_error(
                lhs_ty.to_string(),
                rhs_ty.to_string(),
                operator.line,
                operator.col,
            ));
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
            Type::Reference(inner) => *inner,
            Type::RefMut(inner) => *inner,
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
            return self.generate_associated_value_access(type_id, field);
        }

        if let Some((idx, (_fname, ftype))) = struct_def
            .fields
            .iter()
            .enumerate()
            .find(|(_i, (fname, _))| fname == &field.name)
        {
            self.ensure_struct_field_visible(struct_def, idx, &field.name, field.line, field.col)?;
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
            self.generate_pattern_matching_ex(&else_arms, exception_ty, false)?;

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
        let target_ty = self.get_semantic_type(&target_type)?;
        let literal = match &expression {
            Expression::IntegerLiteral(value) => Some(*value),
            Expression::Unary { operator, right } if operator.token_type == TokenType::Minus => {
                match right.as_ref() {
                    Expression::IntegerLiteral(value) => value.checked_neg(),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(value) = literal {
            if let Type::Primitive(target) = target_ty
                && target.is_fixed_integer()
            {
                let (min, max) = target.integer_bounds().unwrap();
                if !(i128::from(min)..=i128::from(max)).contains(&value) {
                    return Err(SemanticError::Other(format!(
                        "integer literal {} is outside range of {} ({}..={})",
                        value,
                        target.name(),
                        min,
                        max
                    )));
                }
            }
        }

        let expr_ty = self.generate_expression(expression)?;

        match (&expr_ty, &target_ty) {
            (Type::BoxType(inner_ty), Type::BoxType(target_inner_ty)) => {
                self.generate_wrapper_cast(inner_ty, target_inner_ty, true, &target_ty, line, col)
            }
            (Type::Reference(inner_ty), Type::Reference(target_inner_ty)) => {
                self.generate_wrapper_cast(inner_ty, target_inner_ty, false, &target_ty, line, col)
            }
            (Type::HandleType(inner_ty), Type::HandleType(target_inner_ty)) => {
                self.generate_handle_cast(inner_ty, target_inner_ty, &target_ty, line, col)
            }
            // Numeric casts (spec §15.1.2).
            // - `int as float` lifts via the IntToFloat opcode.
            // - `int as int` and `float as float` are accepted no-ops for consistency.
            // - `float as int` is intentionally NOT supported here; callers must use the
            //   `float_to_int_checked(x: float) -> ?int` prelude function to handle NaN,
            //   infinity and out-of-range explicitly.
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Float)) => {
                self.builder.int_to_float();
                Ok(target_ty.clone())
            }
            (Type::Primitive(source), Type::Primitive(target))
                if source.is_integer() && target.is_integer() =>
            {
                if target.is_fixed_integer() {
                    let (min, max) = target.integer_bounds().unwrap();
                    self.builder.check_int_range(min, max);
                }
                Ok(target_ty.clone())
            }
            (Type::Primitive(source), Type::Primitive(PrimitiveType::Float))
                if source.is_fixed_integer() =>
            {
                self.builder.int_to_float();
                Ok(target_ty.clone())
            }
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Int))
            | (Type::Primitive(PrimitiveType::Float), Type::Primitive(PrimitiveType::Float)) => {
                Ok(target_ty.clone())
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

    fn generate_handle_cast(
        &mut self,
        inner_ty: &Type,
        target_inner_ty: &Type,
        target_ty: &Type,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        let (Type::Proto(source_id), Type::Proto(target_id)) = (inner_ty, target_inner_ty) else {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("handle<{}>", self.type_to_string(inner_ty)),
                rhs: format!("handle<{}>", self.type_to_string(target_inner_ty)),
                pos: self.make_pos(line, col),
            });
        };

        if self.proto_is_subtype(*source_id, *target_id) {
            return Ok(target_ty.clone());
        }
        if !self.proto_is_subtype(*target_id, *source_id) {
            return Err(SemanticError::Other(format!(
                "Cast from unrelated foreign proto handles '{}' to '{}' is not supported at line {} column {}",
                self.type_to_string(inner_ty),
                self.type_to_string(target_inner_ty),
                line,
                col
            )));
        }

        let source_tag = self.foreign_proto_tag(*source_id).map(str::to_string);
        let target_tag = self.foreign_proto_tag(*target_id).map(str::to_string);
        let (Some(_source_tag), Some(target_tag)) = (source_tag, target_tag) else {
            return Err(SemanticError::Other(format!(
                "Checked handle downcasts require related @foreign protos, found '{}' and '{}'",
                self.type_to_string(inner_ty),
                self.type_to_string(target_inner_ty)
            )));
        };
        let tag_id = self.add_string_constant(&target_tag);
        self.builder.foreign_downcast(tag_id);
        Ok(target_ty.clone())
    }
}
