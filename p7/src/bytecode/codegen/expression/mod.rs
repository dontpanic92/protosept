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
            Type::BoxType(inner) | Type::Reference(inner) | Type::MutableReference(inner) => {
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
            Expression::StringLiteral(value) => self.generate_string_literal(&value),
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
}
