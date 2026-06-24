use crate::ast::{Expression, Identifier};
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::{
    bytecode::Instruction,
    semantic::{PrimitiveType, Type, TypeDefinition},
};

use super::{Generator, SaResult};

mod literals;
mod operations;

impl Generator {
    /// Creates a SourcePos from line and column numbers, including the current module path
    fn make_pos(&self, line: usize, col: usize) -> Option<SourcePos> {
        Some(SourcePos {
            line,
            col,
            module: Some(self._current_module_path.to_string()),
        })
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

    /// Extracts struct type ID from a type, handling direct struct, boxed struct,
    /// and reference/mutable-reference to struct
    fn extract_struct_type_id(&self, ty: &Type, field: &Identifier) -> SaResult<u32> {
        match ty {
            Type::Struct(type_id) => Ok(*type_id),
            Type::BoxType(inner) | Type::Reference(inner) | Type::RefMut(inner) => {
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
            self.check_struct_conformance(type_id, &[proto_id], &[Vec::new()], line, col)?;
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
            Expression::StringLiteral(value) => self.generate_string_literal(&value),
            Expression::InterpolatedString { parts } => self.generate_interpolated_string(parts),
            Expression::BooleanLiteral(value) => {
                self.builder.ldi(if value { 1 } else { 0 });
                Ok(Type::Primitive(PrimitiveType::Bool))
            }
            Expression::Unary { operator, right } => self.generate_unary(operator, *right),
            Expression::Ref(expr) => self.generate_ref(*expr),
            Expression::RefMut(expr) => self.generate_refmut(*expr),
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
            Expression::ForIn {
                index_var,
                value_var,
                iterable,
                body,
                pos,
            } => self.generate_for_in(index_var, value_var, *iterable, *body, pos),
            Expression::Break { value, pos } => self.generate_break(value, pos),
            Expression::Continue { pos } => self.generate_continue(pos),
            Expression::ArrayLiteral { elements, pos } => {
                self.generate_array_literal(elements, pos, None)
            }
            Expression::ArrayIndex { array, index, pos } => {
                self.generate_array_index(*array, *index, pos)
            }
            Expression::NullLiteral => Err(SemanticError::Other(
                "null literal requires a nullable expected type".to_string(),
            )),
            Expression::ForceUnwrap { operand, token } => {
                self.generate_force_unwrap(*operand, token)
            }
            Expression::Closure {
                parameters,
                body,
                pos,
            } => self.generate_closure(parameters, *body, pos),
            Expression::TupleLiteral { elements, pos } => {
                self.generate_tuple_literal(elements, pos)
            }
            Expression::StructUpdate {
                struct_name,
                base,
                updates,
                pos,
            } => self.generate_struct_update(*struct_name, *base, updates, pos),
            Expression::MapLiteral { pairs, pos } => self.generate_map_literal(pairs, pos),
        }
    }

    /// Generate an expression with an expected type hint.
    /// This is used to infer types for empty array literals and null literals,
    /// and to drive checking-context coercions such as `T -> ?T` widening.
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
                    }
                    return Err(SemanticError::TypeMismatch {
                        lhs: "null".to_string(),
                        rhs: self.type_to_string(expected_ty),
                        pos: None,
                    });
                }
                Err(SemanticError::Other(
                    "null literal requires a nullable expected type".to_string(),
                ))
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
            // and then apply checking-context coercions based on the expected type.
            other => {
                // SAM coercion (§L2): a closure literal at a `box<P>`
                // expected-type site elaborates to an anonymous
                // `struct[P]` + impl with the closure body as the proto's
                // single method. The elaboration emits the method body
                // out-of-line and leaves the call-site value (a
                // `box<P>`) on the stack.
                if let (
                    Expression::Closure {
                        parameters,
                        body,
                        pos,
                    },
                    Some(Type::BoxType(inner)),
                ) = (&other, expected_type)
                    && let Type::Proto(proto_id) = inner.as_ref()
                {
                    let parameters_cloned = parameters.clone();
                    let body_cloned = (**body).clone();
                    let pos_cloned = *pos;
                    let proto_id = *proto_id;
                    if let Some(ty) = self.try_elaborate_sam_closure(
                        parameters_cloned,
                        body_cloned,
                        pos_cloned,
                        proto_id,
                    )? {
                        return Ok(ty);
                    }
                    // Fall through to ordinary closure codegen on
                    // ineligible target (function-typed expected type,
                    // multi-method proto, signature mismatch, etc.).
                }
                let actual_ty = self.generate_expression(other)?;
                if let Some(expected_ty) = expected_type {
                    if let Type::Nullable(inner) = expected_ty
                        && !matches!(actual_ty, Type::Nullable(_))
                        && self.types_compatible(&actual_ty, inner)
                    {
                        // Implicit `T -> ?T` widening at checking/expected-type sites.
                        self.builder.wrap_nullable();
                        return Ok(expected_ty.clone());
                    }

                    // Implicit `box<T> -> box<P>` / `ref<T> -> ref<P>` coercion at
                    // checking/expected-type sites when T declares conformance to
                    // P (§18.5, §18.6). Emits the same instruction as the explicit
                    // `as` cast so dispatch through the proto box/ref works
                    // identically.
                    if let Some(()) =
                        self.try_emit_implicit_proto_coercion(&actual_ty, expected_ty)?
                    {
                        return Ok(expected_ty.clone());
                    }
                }
                Ok(actual_ty)
            }
        }
    }

    /// Try to emit an implicit `box<T> -> box<P>` or `ref<T> -> ref<P>`
    /// proto coercion. Returns `Ok(Some(()))` when the coercion was
    /// emitted, `Ok(None)` when the type pair doesn't match this rule
    /// (including the case where `T` only *structurally* satisfies `P`
    /// without declaring it in its conformance bracket — that case
    /// requires an explicit `as box<P>` cast per the language design).
    ///
    /// A third form, `T -> box<P>` (auto-boxing a bare struct/enum
    /// *value* into an owned proto handle), is also accepted at
    /// checking-context sites. Unlike the spec's reinterpreting
    /// `box<T> -> box<P>` coercion, this one allocates a fresh box for
    /// the temporary before reinterpreting it. This is the affordance
    /// that lets declarative UI children be written as
    /// `children = [Text(...), Button(...)]` (expected
    /// `array<box<Element>>`) without an explicit `box(...)` per element.
    fn try_emit_implicit_proto_coercion(
        &mut self,
        actual: &Type,
        expected: &Type,
    ) -> SaResult<Option<()>> {
        let (is_box, needs_box_alloc, actual_inner, expected_inner) = match (actual, expected) {
            (Type::BoxType(a), Type::BoxType(e)) => (true, false, a.as_ref(), e.as_ref()),
            (Type::Reference(a), Type::Reference(e)) => (false, false, a.as_ref(), e.as_ref()),
            // Auto-box: bare struct/enum value -> owned proto handle.
            (Type::Struct(_) | Type::Enum(_), Type::BoxType(e)) => (true, true, actual, e.as_ref()),
            _ => return Ok(None),
        };

        let proto_id = match expected_inner {
            Type::Proto(pid) => *pid,
            Type::ProtoGeneric { base, .. } => *base,
            _ => return Ok(None),
        };

        let type_id = match actual_inner {
            Type::Struct(sid) => *sid,
            Type::Enum(eid) => *eid,
            _ => return Ok(None),
        };

        // Implicit coercion fires *only* when the type lists `P` in its
        // conformance bracket (`struct[P] T(...)` / `enum[P] T(...)`),
        // per §18.5 / §18.6. A type that only *structurally* satisfies
        // `P` without declaring it must use an explicit `as box<P>` /
        // `as ref<P>` cast (handled by `generate_cast`, which falls back
        // to structural conformance check inside
        // `generate_wrapper_to_proto_cast`).
        let listed = match self.symbol_table.types.get(type_id as usize) {
            Some(TypeDefinition::Struct(s)) => s.conforming_to.contains(&proto_id),
            Some(TypeDefinition::Enum(e)) => e.conforming_to.contains(&proto_id),
            _ => false,
        };
        if !listed {
            return Ok(None);
        }

        // Auto-box form: the bare value is on the stack; allocate a box
        // for it before reinterpreting it as a proto handle.
        if needs_box_alloc {
            self.builder.box_alloc();
        }
        self.generate_wrapper_to_proto_cast(type_id, proto_id, is_box, 0, 0)?;
        Ok(Some(()))
    }
}
