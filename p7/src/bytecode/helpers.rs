use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::{
    ast::{Expression, Identifier, Pattern, Statement},
    semantic::{PrimitiveType, Symbol, SymbolKind, Type, TypeId, UserDefinedType},
};

use super::codegen::{Generator, SaResult};

impl Generator {
    /// Helper to add a string constant to the pool and return its index
    pub(super) fn add_string_constant(&mut self, s: String) -> u32 {
        if let Some(idx) = self
            .string_constants
            .iter()
            .position(|existing| existing == &s)
        {
            idx as u32
        } else {
            let idx = self.string_constants.len() as u32;
            self.string_constants.push(s);
            idx
        }
    }

    /// Resolve a primitive type to its corresponding symbol ID for method lookup
    /// The primitive type symbol is registered during generator initialization
    pub(super) fn resolve_primitive_to_symbol_id(&mut self, prim_ty: &PrimitiveType) -> Option<u32> {
        let type_name = match prim_ty {
            PrimitiveType::String => "string",
            _ => return None, // Other primitive types don't have methods yet
        };

        self.try_load_builtin_type(type_name);

        println!("Resolving primitive type '{}' to symbol ID", type_name);
        println!("Current symbol table: {:?}", self.symbol_table.symbols);
        println!("Current symbol table types: {:?}", self.symbol_table.types);
        // Look up the primitive type symbol directly
        self.symbol_table.find_symbol_in_scope(type_name)
    }

    /// Helper to mark a variable as moved
    pub(super) fn mark_variable_moved(&mut self, var_id: u32) {
        self.moved_variables.insert(var_id);
    }

    /// Helper to check if a variable has been moved
    pub(super) fn is_variable_moved(&self, var_id: u32) -> bool {
        self.moved_variables.contains(&var_id)
    }

    /// Helper to clear moved variables when entering a new function scope
    pub(super) fn clear_moved_variables(&mut self) {
        self.moved_variables.clear();
    }

    pub(super) fn bind_pattern_variable(
        &mut self,
        pattern_name: &Option<Identifier>,
        value_type: Type,
    ) -> SaResult<()> {
        if let Some(name) = pattern_name {
            let var_id = self
                .local_scope
                .as_mut()
                .unwrap()
                .add_variable(name.name.clone(), value_type, false) // Pattern bindings are immutable
                .map_err(|_| SemanticError::VariableOutsideFunction {
                    name: name.name.clone(),
                    pos: Some(SourcePos {
                        line: name.line,
                        col: name.col,
                    }),
                })?;
            self.builder.stvar(var_id);
        } else {
            // No name binding, pop the value
            self.builder.pop();
        }
        Ok(())
    }

    /// Helper method to validate and track result type across match arms
    pub(super) fn validate_match_arm_type(
        &self,
        result_ty: &mut Option<Type>,
        arm_ty: Type,
    ) -> SaResult<()> {
        if let Some(expected_ty) = result_ty {
            if expected_ty != &arm_ty {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!("{:?}", expected_ty),
                    rhs: format!("{:?}", arm_ty),
                    pos: None,
                });
            }
        } else {
            *result_ty = Some(arm_ty);
        }
        Ok(())
    }

    pub(super) fn pattern_to_expression(&self, pattern: &Pattern) -> SaResult<Expression> {
        match pattern {
            Pattern::Identifier(id) => Ok(Expression::Identifier(id.clone())),
            Pattern::IntegerLiteral(val) => Ok(Expression::IntegerLiteral(*val)),
            Pattern::FloatLiteral(val) => Ok(Expression::FloatLiteral(*val)),
            Pattern::StringLiteral(val) => Ok(Expression::StringLiteral(val.clone())),
            Pattern::BooleanLiteral(val) => Ok(Expression::BooleanLiteral(*val)),
            Pattern::FieldAccess { object, field } => {
                let obj_expr = self.pattern_to_expression(object)?;
                Ok(Expression::FieldAccess {
                    object: Box::new(obj_expr),
                    field: field.clone(),
                })
            }
        }
    }

    pub(super) fn extract_intrinsic_name(attributes: &[crate::ast::Attribute]) -> Option<String> {
        for attr in attributes {
            if attr.name.name == "intrinsic" {
                // Look for the intrinsic name in the arguments
                for (name_opt, expr) in &attr.arguments {
                    // Check if this is a positional argument (first arg) or named "name"
                    let is_target = name_opt.as_ref().map_or(true, |n| n.name == "name");
                    if is_target {
                        if let Expression::StringLiteral(s) = expr {
                            return Some(s.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Resolve a protocol identifier to its TypeId
    pub(super) fn resolve_proto_identifier(
        &self,
        proto_name: &Identifier,
    ) -> SaResult<crate::semantic::TypeId> {
        let proto_type = self
            .symbol_table
            .find_type_in_scope(&proto_name.name)
            .ok_or_else(|| SemanticError::TypeNotFound {
                name: proto_name.name.clone(),
                pos: Some(SourcePos {
                    line: proto_name.line,
                    col: proto_name.col,
                }),
            })?;

        match proto_type {
            Type::Proto(proto_id) => Ok(proto_id),
            _ => Err(SemanticError::Other(format!(
                "Expected protocol name, found type '{}' at line {} column {}",
                proto_name.name, proto_name.line, proto_name.col
            ))),
        }
    }
}
