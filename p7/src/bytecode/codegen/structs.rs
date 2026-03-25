use crate::ast::{Expression, FunctionCall, Identifier};
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::semantic::{Type, TypeDefinition, TypeId};

use super::{Generator, SaResult};

impl Generator {
    pub(super) fn generate_struct_from_call(
        &mut self,
        call: FunctionCall,
        type_id: TypeId,
    ) -> SaResult<Type> {
        // Get struct definition
        let (call_name, (call_line, call_col)) = (call.callee.get_name(), call.callee.get_pos());

        let struct_def = match self.symbol_table.get_type(type_id) {
            TypeDefinition::Struct(s) => s.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "Struct".to_string(),
                    rhs: "Non-struct type".to_string(),
                    pos: SourcePos::at(call_line, call_col),
                });
            }
        };

        let field_names: Vec<String> = struct_def
            .fields
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let field_defaults: Vec<Option<Expression>> = struct_def.field_defaults.clone();

        // Process arguments using shared logic
        let ordered_exprs = self.process_arguments(
            &call_name,
            call_line,
            call_col,
            call.arguments,
            &field_names,
            &field_defaults,
        )?;

        self.push_argument_list(ordered_exprs)?;
        self.builder.newstruct(struct_def.fields.len() as u32);

        Ok(Type::Struct(type_id))
    }

    /// Generate struct update: Type(...base, field1 = val1, field2 = val2)
    pub(super) fn generate_struct_update(
        &mut self,
        struct_name_expr: Expression,
        base: Expression,
        updates: Vec<(Identifier, Expression)>,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        // Resolve struct type from the name expression
        let type_name = struct_name_expr.get_name();
        let struct_type = self.resolve_qualified_type_name(&type_name, pos.0, pos.1)
            .or_else(|_| self.require_type_in_scope(&type_name, pos.0, pos.1))?;

        let type_id = match struct_type {
            Type::Struct(id) => id,
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "Struct type".to_string(),
                    rhs: format!("'{}' is not a struct", type_name),
                    pos: SourcePos::at(pos.0, pos.1),
                });
            }
        };

        let struct_def = match self.symbol_table.get_type(type_id) {
            TypeDefinition::Struct(s) => s.clone(),
            _ => return Err(SemanticError::Other("Expected struct type definition".to_string())),
        };

        // Build a map of field name → update expression
        let mut update_map: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
        for (field_name, expr) in updates {
            if update_map.contains_key(&field_name.name) {
                return Err(SemanticError::Other(format!(
                    "Duplicate field '{}' in struct update", field_name.name
                )));
            }
            // Validate field exists
            if !struct_def.fields.iter().any(|(f, _)| f == &field_name.name) {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!("Struct '{}'", type_name),
                    rhs: format!("Unknown field '{}'", field_name.name),
                    pos: SourcePos::at(field_name.line, field_name.col),
                });
            }
            update_map.insert(field_name.name, expr);
        }

        // Generate base expression and store in a temporary variable
        let base_ty = self.generate_expression(base)?;
        let base_var = self.local_scope.as_mut().unwrap()
            .add_variable("$struct_update_base".to_string(), base_ty, false)
            .map_err(|_| SemanticError::Other("Cannot create struct update temp".to_string()))?;
        self.builder.stvar(base_var);

        // For each field: use update expression if provided, else load from base
        for (field_idx, (field_name, field_type)) in struct_def.fields.iter().enumerate() {
            if let Some(update_expr) = update_map.remove(field_name) {
                let expr_ty = self.generate_expression(update_expr)?;
                if !self.types_compatible(&expr_ty, field_type) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("field '{}' expects {}", field_name, field_type.to_string()),
                        rhs: format!("got {}", expr_ty.to_string()),
                        pos: SourcePos::at(pos.0, pos.1),
                    });
                }
            } else {
                // Load from base struct
                self.builder.ldvar(base_var);
                self.builder.ldfield(field_idx as u32);
            }
        }

        self.builder.newstruct(struct_def.fields.len() as u32);
        Ok(Type::Struct(type_id))
    }
}
