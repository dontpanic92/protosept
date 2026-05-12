use crate::ast::{Expression, Identifier};
use crate::errors::SemanticError;
use crate::semantic::{Type, TypeDefinition, TypeId};

use super::{Generator, SaResult};

impl Generator {
    pub(super) fn generate_enum_variant_from_call(
        &mut self,
        callee_expr: Expression,
        arguments: Vec<(Option<Identifier>, Expression)>,
        enum_type_id: TypeId,
    ) -> SaResult<Type> {
        // Extract the variant name from the field access
        let variant_name = if let Expression::FieldAccess { object: _, field } = &callee_expr {
            field.name.clone()
        } else {
            return Err(SemanticError::Other(
                "Invalid enum variant construction".to_string(),
            ));
        };

        // Get enum definition
        let enum_def = match self.symbol_table.get_type(enum_type_id) {
            TypeDefinition::Enum(e) => e.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "Enum".to_string(),
                    rhs: "Non-enum type".to_string(),
                    pos: callee_expr.source_pos(),
                });
            }
        };

        // Find the variant
        let variant_opt = enum_def
            .variants
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == &variant_name);

        let (variant_index, field_types) = if let Some((idx, (_, types))) = variant_opt {
            (idx, types.clone())
        } else {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Enum '{}'", enum_def.qualified_name),
                rhs: format!("Unknown variant '{}'", variant_name),
                pos: callee_expr.source_pos(),
            });
        };

        // Check if this is a payload variant
        if field_types.is_empty() {
            // Unit variant called like a function - this is an error
            return Err(SemanticError::TypeMismatch {
                lhs: format!("Unit variant '{}'", variant_name),
                rhs: "Cannot call unit variant with arguments".to_string(),
                pos: callee_expr.source_pos(),
            });
        }

        let (call_line, call_col) = callee_expr.get_pos();
        let ordered_exprs = self.process_positional_arguments(
            &variant_name,
            call_line,
            call_col,
            arguments,
            field_types.len(),
        )?;

        // For payload variants, generate code to create the enum value
        // First, push the variant index
        self.builder.ldi(variant_index as i64);

        // Then push all the field values
        self.push_typed_argument_list(ordered_exprs, &field_types, call_line, call_col)?;

        // Create the enum value with the variant index and fields
        // We represent enum values as structs where the first field is the variant index
        // and subsequent fields are the payload values: [variant_index, field1, field2, ...]
        self.builder.newstruct((field_types.len() + 1) as u32);

        Ok(Type::Enum(enum_type_id))
    }
}
