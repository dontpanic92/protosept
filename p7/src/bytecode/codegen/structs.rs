use crate::ast::{Expression, FunctionCall};
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
}
