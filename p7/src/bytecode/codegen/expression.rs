use crate::ast::{Expression, FunctionCall, Identifier, InterpolatedStringPart};
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::lexer::Token;
use crate::{
    bytecode::Instruction,
    lexer::TokenType,
    semantic::{PrimitiveType, Type, TypeDefinition},
};

use super::{Generator, SaResult};

impl Generator {
    /// Creates a SourcePos from line and column numbers
    fn make_pos(&self, line: usize, col: usize) -> Option<SourcePos> {
        Some(SourcePos { line, col })
    }

    /// Creates a type mismatch error
    fn type_mismatch_error(
        &self,
        lhs: String,
        rhs: String,
        line: usize,
        col: usize,
    ) -> SemanticError {
        SemanticError::TypeMismatch {
            lhs,
            rhs,
            pos: self.make_pos(line, col),
        }
    }

    /// Validates that a type is boolean, returns error if not
    pub(super) fn expect_bool_type(&self, ty: &Type, line: usize, col: usize) -> SaResult<()> {
        if *ty != Type::Primitive(PrimitiveType::Bool) {
            return Err(self.type_mismatch_error(ty.to_string(), "bool".to_string(), line, col));
        }
        Ok(())
    }

    /// Extracts struct type ID from a type, handling both direct struct and boxed struct
    fn extract_struct_type_id(&self, ty: &Type, field: &Identifier) -> SaResult<u32> {
        match ty {
            Type::Struct(type_id) => Ok(*type_id),
            Type::BoxType(inner) => {
                if let Type::Struct(type_id) = **inner {
                    Ok(type_id)
                } else {
                    Err(self.type_mismatch_error(
                        ty.to_string(),
                        "Struct or box<Struct>".to_string(),
                        field.line,
                        field.col,
                    ))
                }
            }
            _ => Err(self.type_mismatch_error(
                ty.to_string(),
                "Struct or box<Struct>".to_string(),
                field.line,
                field.col,
            )),
        }
    }

    /// Checks conformance and generates cast instruction for box/ref to proto casts
    fn generate_wrapper_to_proto_cast(
        &mut self,
        type_id: u32,
        proto_id: u32,
        is_box: bool,
        line: usize,
        col: usize,
    ) -> SaResult<()> {
        // Get conforming_to list based on type
        let conforms = match &self.symbol_table.types[type_id as usize] {
            TypeDefinition::Struct(s) => s.conforming_to.contains(&proto_id),
            TypeDefinition::Enum(e) => e.conforming_to.contains(&proto_id),
            _ => {
                return Err(SemanticError::Other(
                    "Expected struct or enum type".to_string(),
                ));
            }
        };

        if !conforms {
            self.check_struct_conformance(type_id, &[proto_id], line, col)?;
        }

        // Generate appropriate instruction
        if is_box {
            self.builder
                .add_instruction(Instruction::BoxToProto(type_id, proto_id));
        } else {
            self.builder
                .add_instruction(Instruction::RefToProto(type_id, proto_id));
        }

        Ok(())
    }

    pub(super) fn generate_expression(&mut self, expression: Expression) -> SaResult<Type> {
        match expression {
            Expression::Identifier(identifier) => self.generate_identifier(identifier),
            Expression::IntegerLiteral(value) => {
                self.builder.ldi(value);
                Ok(Type::Primitive(PrimitiveType::Int))
            }
            Expression::FloatLiteral(value) => {
                self.builder.ldf(value);
                Ok(Type::Primitive(PrimitiveType::Float))
            }
            Expression::StringLiteral(value) => self.generate_string_literal(value),
            Expression::InterpolatedString { parts } => self.generate_interpolated_string(parts),
            Expression::BooleanLiteral(value) => {
                self.builder.ldi(if value { 1 } else { 0 });
                Ok(Type::Primitive(PrimitiveType::Bool))
            }
            Expression::Unary { operator, right } => self.generate_unary(operator, *right),
            Expression::Ref(expr) => self.generate_ref(*expr),
            Expression::Binary {
                left,
                operator,
                right,
            } => self.generate_binary(*left, operator, *right),
            Expression::If {
                condition,
                then_branch,
                else_branch,
                pos,
            } => self.generate_if(*condition, *then_branch, else_branch.map(|e| *e), pos),
            Expression::FunctionCall(call) => self.generate_function_call(call),
            Expression::FieldAccess { object, field } => self.generate_field_access(*object, field),
            Expression::Block(statements) => self.generate_block(statements, vec![]),
            Expression::Try {
                try_block,
                else_arms,
            } => self.generate_try(*try_block, else_arms),
            Expression::Match { scrutinee, arms } => self.generate_match(*scrutinee, arms),
            Expression::BlockValue(expression) => self.generate_expression(*expression),
            Expression::Cast {
                expression,
                target_type,
            } => self.generate_cast(*expression, target_type),
            Expression::GenericInstantiation { base, .. } => Err(SemanticError::TypeMismatch {
                lhs: "expression value".to_string(),
                rhs: format!("generic type instantiation '{}'", base.name),
                pos: self.make_pos(base.line, base.col),
            }),
            Expression::Loop { body, .. } => self.generate_loop(*body),
            Expression::While {
                condition,
                body,
                pos,
            } => self.generate_while(*condition, *body, pos),
            Expression::Break { value, pos } => self.generate_break(value, pos),
            Expression::Continue { pos } => self.generate_continue(pos),
            Expression::ArrayLiteral { elements, pos } => {
                self.generate_array_literal(elements, pos, None)
            }
            Expression::ArrayIndex { array, index, pos } => {
                self.generate_array_index(*array, *index, pos)
            }
            Expression::NullLiteral => {
                // Null literal needs type context to determine the inner type
                // For now, emit a placeholder - the actual type comes from bidirectional typing
                self.builder.ldnull();
                // Return a placeholder nullable type; the actual type will be refined by context
                Ok(Type::Nullable(Box::new(Type::Primitive(
                    PrimitiveType::Unit,
                ))))
            }
            Expression::ForceUnwrap { operand, token } => {
                self.generate_force_unwrap(*operand, token)
            }
        }
    }

    fn generate_interpolated_string(
        &mut self,
        parts: Vec<InterpolatedStringPart>,
    ) -> SaResult<Type> {
        let concat_id = self.add_string_constant("string.concat".to_string());
        let mut has_value = false;

        for part in parts {
            let part_ty = match part {
                InterpolatedStringPart::Literal(value) => self.generate_string_literal(value)?,
                InterpolatedStringPart::Expr(expr) => {
                    let display_call = Expression::FunctionCall(FunctionCall {
                        callee: Box::new(Expression::FieldAccess {
                            object: Box::new(expr),
                            field: Identifier {
                                name: "display".to_string(),
                                line: 0,
                                col: 0,
                            },
                        }),
                        arguments: vec![],
                    });

                    self.generate_expression(display_call)?
                }
            };

            if part_ty != Type::Primitive(PrimitiveType::String) {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&part_ty),
                    rhs: "string".to_string(),
                    pos: None,
                });
            }

            if has_value {
                self.builder.call_host_function(concat_id);
            } else {
                has_value = true;
            }
        }

        if !has_value {
            let empty_id = self.add_string_constant(String::new());
            self.builder.lds(empty_id);
        }

        Ok(Type::Primitive(PrimitiveType::String))
    }

    /// Generate an expression with an expected type hint.
    /// This is used to infer types for empty array literals and null literals.
    pub(super) fn generate_expression_with_expected_type(
        &mut self,
        expression: Expression,
        expected_type: Option<&Type>,
    ) -> SaResult<Type> {
        match expression {
            Expression::NullLiteral => {
                // Null literal needs expected type to determine inner type
                if let Some(expected_ty) = expected_type {
                    if let Type::Nullable(_) = expected_ty {
                        self.builder.ldnull();
                        return Ok(expected_ty.clone());
                    } else {
                        // If expected type is not nullable, this is an error
                        return Ok(Type::Nullable(Box::new(Type::Primitive(
                            PrimitiveType::Unit,
                        ))));
                    }
                }
                // No expected type - return generic nullable
                self.builder.ldnull();
                Ok(Type::Nullable(Box::new(Type::Primitive(
                    PrimitiveType::Unit,
                ))))
            }
            Expression::ArrayLiteral { elements, pos } => {
                // Extract element type from expected array type
                let expected_element_type = expected_type.and_then(|ty| {
                    if let Type::Array(elem_ty) = ty {
                        Some(elem_ty.as_ref())
                    } else {
                        None
                    }
                });
                self.generate_array_literal(elements, pos, expected_element_type)
            }
            // For all other expressions, delegate to the regular generate_expression
            other => self.generate_expression(other),
        }
    }

    fn generate_identifier(&mut self, identifier: Identifier) -> SaResult<Type> {
        // Handle `Self` keyword for type references in methods
        if identifier.name == "Self" {
            if let Some(self_type) = &self.current_self_type {
                return Ok(self_type.clone());
            } else {
                return Err(SemanticError::Other(
                    "Self can only be used inside methods".to_string(),
                ));
            }
        }

        // Try to find as local variable
        if let Some(var_id) = self
            .local_scope
            .as_mut()
            .unwrap()
            .find_variable(&identifier.name)
        {
            if self.is_variable_moved(var_id) {
                return Err(SemanticError::UseAfterMove {
                    name: identifier.name,
                    pos: self.make_pos(identifier.line, identifier.col),
                });
            }
            self.builder.ldvar(var_id);
            let ty = self.local_scope.as_mut().unwrap().get_variable_type(var_id);
            return Ok(ty);
        }

        // Try to find as parameter
        if let Some(param_id) = self
            .local_scope
            .as_mut()
            .unwrap()
            .find_param(&identifier.name)
        {
            if self.is_variable_moved(param_id) {
                return Err(SemanticError::UseAfterMove {
                    name: identifier.name,
                    pos: self.make_pos(identifier.line, identifier.col),
                });
            }
            self.builder.ldpar(param_id);
            let ty = self.local_scope.as_mut().unwrap().get_param_type(param_id);
            return Ok(ty);
        }

        Err(SemanticError::VariableNotFound {
            name: identifier.name,
            pos: self.make_pos(identifier.line, identifier.col),
        })
    }

    fn generate_string_literal(&mut self, value: String) -> SaResult<Type> {
        let string_index = if let Some(idx) = self.string_constants.iter().position(|s| s == &value)
        {
            idx as u32
        } else {
            let idx = self.string_constants.len() as u32;
            self.string_constants.push(value);
            idx
        };

        self.builder.lds(string_index);
        Ok(Type::Primitive(PrimitiveType::String))
    }

    fn generate_unary(&mut self, operator: Token, right: Expression) -> SaResult<Type> {
        let ty = self.generate_expression(right)?;
        match operator.token_type {
            TokenType::Minus => {
                self.builder.neg();
                Ok(ty)
            }
            TokenType::Not => {
                self.builder.not();
                Ok(ty)
            }
            TokenType::Multiply => self.generate_deref(ty, &operator),
            _ => unimplemented!(),
        }
    }

    fn generate_deref(&mut self, ty: Type, operator: &Token) -> SaResult<Type> {
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

    fn generate_ref(&mut self, expr: Expression) -> SaResult<Type> {
        // Special case: ref(*b) where b is a box
        if let Expression::Unary { operator, right } = &expr {
            if operator.token_type == TokenType::Multiply {
                let inner_ty = self.generate_expression((**right).clone())?;
                if let Type::BoxType(boxed_inner) = inner_ty {
                    return Ok(Type::Reference(boxed_inner));
                }
            }
        }

        let ty = self.generate_expression(expr)?;

        if matches!(ty, Type::Reference(_)) {
            return Err(SemanticError::Other("Cannot take ref of ref".to_string()));
        }

        Ok(Type::Reference(Box::new(ty)))
    }

    fn generate_force_unwrap(&mut self, operand: Expression, token: Token) -> SaResult<Type> {
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

    fn generate_binary(
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
            _ => unimplemented!("assignment to this lvalue is not implemented"),
        }
    }

    fn generate_assignment_to_identifier(
        &mut self,
        identifier: Identifier,
        rhs: Expression,
    ) -> SaResult<Type> {
        let rhs_ty = self.generate_expression(rhs)?;

        // Try local variable first
        if let Some(var_id) = self
            .local_scope
            .as_mut()
            .unwrap()
            .find_variable(&identifier.name)
        {
            let lhs_ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);

            if matches!(lhs_ty, Type::Reference(_)) {
                return Err(SemanticError::Other(format!(
                    "Cannot assign to read-only ref '{}'",
                    identifier.name
                )));
            }

            if !self
                .local_scope
                .as_ref()
                .unwrap()
                .is_variable_mutable(var_id)
            {
                return Err(SemanticError::Other(format!(
                    "Cannot assign to immutable variable '{}' (use 'var' instead of 'let')",
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
        if let Some(param_id) = self
            .local_scope
            .as_mut()
            .unwrap()
            .find_param(&identifier.name)
        {
            let lhs_ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);

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

        Err(SemanticError::VariableNotFound {
            name: identifier.name.clone(),
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

    fn generate_binary_operation(
        &mut self,
        left: Expression,
        operator: Token,
        right: Expression,
    ) -> SaResult<Type> {
        let lhs_ty = self.generate_expression(left)?;
        let rhs_ty = self.generate_expression(right)?;

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
                (Type::Nullable(_), Type::Nullable(_)) if is_equality => {
                    lhs_ty.clone()
                }
                // Allow string + string for concatenation
                (Type::Primitive(PrimitiveType::String), Type::Primitive(PrimitiveType::String))
                    if operator.token_type == TokenType::Plus =>
                {
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
            let host_fn_idx = self.add_string_constant("string.concat".to_string());
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

    fn generate_if(
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

        self.generate_expression(then_branch)?;

        if let Some(else_branch) = else_branch {
            let jump_to_skip_else_placeholder = self.builder.next_address();
            self.builder.jmp(0);

            let else_branch_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_if_false_placeholder, else_branch_address);

            self.generate_expression(else_branch)?;

            let end_of_if_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_to_skip_else_placeholder, end_of_if_address);
        } else {
            let end_of_if_address = self.builder.next_address();
            self.builder
                .patch_jump_address(jump_if_false_placeholder, end_of_if_address);
        }

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_field_access(&mut self, object: Expression, field: Identifier) -> SaResult<Type> {
        let object_name = object.get_name();

        // Resolve object type and determine if it's a static access
        let (object_ty, is_static_access) = self.resolve_field_access_object(&object)?;

        // Auto-deref references and boxes
        let object_ty = match object_ty {
            Type::Reference(inner) => *inner,
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
                    name: base.name.clone(),
                    pos: self.make_pos(base.line, base.col),
                });
            }
        }

        // Check for type identifier (static access)
        if let Expression::Identifier(ref identifier) = *object {
            if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
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

    fn generate_try(
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

    fn generate_match(
        &mut self,
        scrutinee: Expression,
        arms: Vec<crate::ast::MatchArm>,
    ) -> SaResult<Type> {
        let scrutinee_ty = self.generate_expression(scrutinee)?;
        self.generate_pattern_matching(&arms, scrutinee_ty)
    }

    fn generate_cast(
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

    fn generate_array_literal(
        &mut self,
        elements: Vec<Expression>,
        pos: (usize, usize),
        expected_element_type: Option<&Type>,
    ) -> SaResult<Type> {
        let (line, col) = pos;

        // Infer element type from first element if non-empty, or use expected type for empty arrays
        let element_type = if elements.is_empty() {
            // Empty array uses expected type from context
            if let Some(expected) = expected_element_type {
                expected.clone()
            } else {
                return Err(SemanticError::Other(format!(
                    "Cannot infer type for empty array literal at {}:{} - expected type annotation required",
                    line, col
                )));
            }
        } else {
            // Generate code for all elements and check they have the same type
            let first_expr_type = self.generate_expression(elements[0].clone())?;

            for element in &elements[1..] {
                let expr_type = self.generate_expression(element.clone())?;
                if expr_type != first_expr_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(&first_expr_type),
                        rhs: self.type_to_string(&expr_type),
                        pos: self.make_pos(line, col),
                    });
                }
            }

            first_expr_type
        };

        // Push element count onto stack
        self.builder.ldi(elements.len() as i64);

        // Call array.new host function
        let string_id = self.add_string_constant("array.new".to_string());
        self.builder.call_host_function(string_id);

        Ok(Type::Array(Box::new(element_type)))
    }

    fn generate_array_index(
        &mut self,
        array: Expression,
        index: Expression,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let (line, col) = pos;

        // Generate code for array expression
        let array_type = self.generate_expression(array)?;

        // Check that it's an array type
        let element_type = match array_type {
            Type::Array(elem_type) => *elem_type,
            Type::Reference(inner) => match *inner {
                Type::Array(elem_type) => *elem_type,
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "array".to_string(),
                        rhs: self.type_to_string(&other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            Type::BoxType(inner) => match *inner {
                Type::Array(elem_type) => {
                    self.builder.box_deref();
                    *elem_type
                }
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "array".to_string(),
                        rhs: self.type_to_string(&other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "array".to_string(),
                    rhs: self.type_to_string(&array_type),
                    pos: self.make_pos(line, col),
                });
            }
        };

        // Generate code for index expression
        let index_type = self.generate_expression(index)?;

        // Check that index is an integer
        if index_type != Type::Primitive(PrimitiveType::Int) {
            return Err(SemanticError::TypeMismatch {
                lhs: "int".to_string(),
                rhs: self.type_to_string(&index_type),
                pos: self.make_pos(line, col),
            });
        }

        // Call array.index host function
        let string_id = self.add_string_constant("array.index".to_string());
        self.builder.call_host_function(string_id);

        Ok(element_type)
    }
}
