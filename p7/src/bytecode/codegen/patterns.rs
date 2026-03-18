use super::{Generator, SaResult};
use crate::ast::{MatchArm, Pattern};
use crate::errors::SemanticError;
use crate::semantic::{PrimitiveType, Type, TypeDefinition};

impl Generator {
    /// Generate pattern matching code for a list of match arms.
    /// The scrutinee value should already be on the stack.
    pub(crate) fn generate_pattern_matching(
        &mut self,
        arms: &[MatchArm],
        scrutinee_ty: Type,
    ) -> SaResult<Type> {
        // Track jump addresses for all arms to jump to end
        let mut end_jumps = Vec::new();
        let mut result_ty = None;

        for (i, arm) in arms.iter().enumerate() {
            let is_last_arm = i == arms.len() - 1;
            let pattern = &arm.pattern.pattern;

            match pattern {
                Pattern::EnumVariant {
                    enum_name,
                    variant_name,
                    sub_patterns,
                } => {
                    // Look up the enum type
                    let enum_type_id = match self.require_type_from_identifier(enum_name)? {
                        Type::Enum(id) => id,
                        _ => {
                            return Err(SemanticError::TypeMismatch {
                                lhs: "Enum type".to_string(),
                                rhs: format!("'{}' is not an enum", enum_name.name),
                                pos: enum_name.pos(),
                            });
                        }
                    };

                    let enum_def = match self.symbol_table.get_type(enum_type_id) {
                        TypeDefinition::Enum(e) => e.clone(),
                        _ => {
                            return Err(SemanticError::Other(
                                "Expected enum type definition".to_string(),
                            ));
                        }
                    };

                    // Find the variant
                    let variant_opt = enum_def
                        .variants
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| name == &variant_name.name);

                    let (variant_index, field_types) =
                        if let Some((idx, (_, types))) = variant_opt {
                            (idx, types.clone())
                        } else {
                            return Err(SemanticError::TypeMismatch {
                                lhs: format!("Enum '{}'", enum_def.qualified_name),
                                rhs: format!("Unknown variant '{}'", variant_name.name),
                                pos: variant_name.pos(),
                            });
                        };

                    // Validate sub_pattern count matches field count
                    if sub_patterns.len() != field_types.len() {
                        return Err(SemanticError::TypeMismatch {
                            lhs: format!(
                                "{} fields in variant '{}'",
                                field_types.len(),
                                variant_name.name
                            ),
                            rhs: format!("{} patterns provided", sub_patterns.len()),
                            pos: variant_name.pos(),
                        });
                    }

                    // Dup scrutinee, load variant tag (field 0), compare
                    self.builder.dup();
                    self.builder.ldfield(0);
                    self.builder.ldi(variant_index as i64);
                    self.builder.eq();
                    self.builder.not();

                    let no_match_jump = self.builder.next_address();
                    self.builder.jif(0); // placeholder

                    // Bind the named pattern variable (if any, e.g. `name: Result.Ok(n)`)
                    if arm.pattern.name.is_some() {
                        self.builder.dup();
                        self.bind_pattern_variable(
                            &arm.pattern.name,
                            scrutinee_ty.clone(),
                        )?;
                    }

                    // Extract and bind each sub-pattern
                    for (field_idx, sub_pat) in sub_patterns.iter().enumerate() {
                        if !sub_pat.is_wildcard() {
                            if let Pattern::Identifier(id) = sub_pat {
                                self.builder.dup();
                                self.builder.ldfield((field_idx + 1) as u32);
                                let field_ty = field_types[field_idx].clone();
                                self.bind_pattern_variable(
                                    &Some(id.clone()),
                                    field_ty,
                                )?;
                            }
                        }
                    }

                    // Generate arm body
                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                    if !is_last_arm {
                        let end_jump = self.builder.next_address();
                        self.builder.jmp(0);
                        end_jumps.push(end_jump);
                    }

                    let next_arm = self.builder.next_address();
                    self.builder.patch_jump_address(no_match_jump, next_arm);
                }

                Pattern::StructPattern {
                    struct_name,
                    field_patterns,
                } => {
                    // Look up the struct type
                    let struct_type_id = match self.require_type_from_identifier(struct_name)? {
                        Type::Struct(id) => id,
                        _ => {
                            return Err(SemanticError::TypeMismatch {
                                lhs: "Struct type".to_string(),
                                rhs: format!("'{}' is not a struct", struct_name.name),
                                pos: struct_name.pos(),
                            });
                        }
                    };

                    let struct_def = match self.symbol_table.get_type(struct_type_id) {
                        TypeDefinition::Struct(s) => s.clone(),
                        _ => {
                            return Err(SemanticError::Other(
                                "Expected struct type definition".to_string(),
                            ));
                        }
                    };

                    // Validate field count
                    if field_patterns.len() != struct_def.fields.len() {
                        return Err(SemanticError::TypeMismatch {
                            lhs: format!(
                                "{} fields in struct '{}'",
                                struct_def.fields.len(),
                                struct_name.name
                            ),
                            rhs: format!("{} patterns provided", field_patterns.len()),
                            pos: struct_name.pos(),
                        });
                    }

                    // Struct patterns are irrefutable, so no tag check needed.
                    // Bind the named pattern variable (if any)
                    if arm.pattern.name.is_some() {
                        self.builder.dup();
                        self.bind_pattern_variable(
                            &arm.pattern.name,
                            scrutinee_ty.clone(),
                        )?;
                    }

                    // Extract and bind each field
                    for (field_idx, sub_pat) in field_patterns.iter().enumerate() {
                        if !sub_pat.is_wildcard() {
                            if let Pattern::Identifier(id) = sub_pat {
                                self.builder.dup();
                                self.builder.ldfield(field_idx as u32);
                                let field_ty = struct_def.fields[field_idx].1.clone();
                                self.bind_pattern_variable(
                                    &Some(id.clone()),
                                    field_ty,
                                )?;
                            }
                        }
                    }

                    // Generate arm body
                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                    if !is_last_arm {
                        let end_jump = self.builder.next_address();
                        self.builder.jmp(0);
                        end_jumps.push(end_jump);
                    }
                }

                _ if pattern.is_wildcard() => {
                    // Wildcard pattern - matches everything
                    self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;
                }

                _ => {
                    // Non-wildcard, non-destructuring: compare with eq
                    self.builder.dup();

                    let pattern_expr = self.pattern_to_expression(&arm.pattern.pattern)?;
                    self.generate_expression(pattern_expr)?;

                    self.builder.eq();
                    self.builder.not();

                    let no_match_jump_placeholder = self.builder.next_address();
                    self.builder.jif(0);

                    self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                    if !is_last_arm {
                        let end_jump_address = self.builder.next_address();
                        self.builder.jmp(0);
                        end_jumps.push(end_jump_address);
                    }

                    let next_arm_address = self.builder.next_address();
                    self.builder
                        .patch_jump_address(no_match_jump_placeholder, next_arm_address);
                }
            }
        }

        // Patch all end jumps to point here
        let end_address = self.builder.next_address();
        for jump_address in end_jumps {
            self.builder.patch_jump_address(jump_address, end_address);
        }

        Ok(result_ty.unwrap_or(Type::Primitive(PrimitiveType::Unit)))
    }
}
