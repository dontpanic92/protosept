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
        // Spec §9.6.4: match must be exhaustive.
        self.check_match_exhaustive(arms, &scrutinee_ty)?;

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
                    // Try direct lookup first, then qualified name for cross-module types
                    let resolved_type =
                        self.require_type_from_identifier(enum_name).or_else(|_| {
                            if enum_name.name.contains('.') {
                                // Already a qualified name (e.g., "types.Direction" from module.Enum.Variant pattern)
                                self.resolve_qualified_type_name(
                                    &enum_name.name,
                                    enum_name.line,
                                    enum_name.col,
                                )
                            } else {
                                // Try "module.TypeName" qualified resolution
                                let qualified = format!("{}.{}", enum_name.name, variant_name.name);
                                self.resolve_qualified_type_name(
                                    &qualified,
                                    enum_name.line,
                                    enum_name.col,
                                )
                            }
                        })?;

                    match resolved_type {
                        Type::Struct(struct_type_id) => {
                            // Cross-module struct pattern: types.Pos(r, c)
                            let struct_def = match self.symbol_table.get_type(struct_type_id) {
                                TypeDefinition::Struct(s) => s.clone(),
                                _ => {
                                    return Err(SemanticError::Other(
                                        "Expected struct type definition".to_string(),
                                    ));
                                }
                            };

                            if sub_patterns.len() != struct_def.fields.len() {
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!(
                                        "{} fields in struct '{}'",
                                        struct_def.fields.len(),
                                        struct_def.qualified_name
                                    ),
                                    rhs: format!("{} patterns provided", sub_patterns.len()),
                                    pos: enum_name.pos(),
                                });
                            }

                            if arm.pattern.name.is_some() {
                                self.builder.dup();
                                self.bind_pattern_variable(
                                    &arm.pattern.name,
                                    scrutinee_ty.clone(),
                                )?;
                            }

                            for (field_idx, sub_pat) in sub_patterns.iter().enumerate() {
                                if !sub_pat.is_wildcard()
                                    && let Pattern::Identifier(id) = sub_pat
                                {
                                    let (field_name, field_ty) =
                                        struct_def.fields[field_idx].clone();
                                    self.ensure_struct_field_visible(
                                        &struct_def,
                                        field_idx,
                                        &field_name,
                                        id.line,
                                        id.col,
                                    )?;
                                    self.builder.dup();
                                    self.builder.ldfield(field_idx as u32);
                                    self.bind_pattern_variable(&Some(id.clone()), field_ty)?;
                                }
                            }

                            let arm_ty = self.generate_expression(arm.expression.clone())?;
                            self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                            if !is_last_arm {
                                let end_jump = self.builder.next_address();
                                self.builder.jmp(0);
                                end_jumps.push(end_jump);
                            }
                        }
                        Type::Enum(enum_type_id) => {
                            // Look up the enum type
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
                                if !sub_pat.is_wildcard()
                                    && let Pattern::Identifier(id) = sub_pat
                                {
                                    self.builder.dup();
                                    self.builder.ldfield((field_idx + 1) as u32);
                                    let field_ty = field_types[field_idx].clone();
                                    self.bind_pattern_variable(&Some(id.clone()), field_ty)?;
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
                        _ => {
                            return Err(SemanticError::TypeMismatch {
                                lhs: "Enum or Struct type".to_string(),
                                rhs: format!(
                                    "'{}.{}' is neither an enum nor a struct",
                                    enum_name.name, variant_name.name
                                ),
                                pos: enum_name.pos(),
                            });
                        }
                    }
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
                        self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;
                    }

                    // Extract and bind each field
                    for (field_idx, sub_pat) in field_patterns.iter().enumerate() {
                        if !sub_pat.is_wildcard()
                            && let Pattern::Identifier(id) = sub_pat
                        {
                            let (field_name, field_ty) = struct_def.fields[field_idx].clone();
                            self.ensure_struct_field_visible(
                                &struct_def,
                                field_idx,
                                &field_name,
                                id.line,
                                id.col,
                            )?;
                            self.builder.dup();
                            self.builder.ldfield(field_idx as u32);
                            self.bind_pattern_variable(&Some(id.clone()), field_ty)?;
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

                Pattern::TuplePattern { sub_patterns } => {
                    // Tuple patterns are irrefutable — no tag check needed.
                    // Validate arity at compile time.
                    let element_types = match &scrutinee_ty {
                        Type::Tuple(types) => types.clone(),
                        _ => {
                            return Err(SemanticError::Other(format!(
                                "Cannot match non-tuple type '{}' with tuple pattern",
                                scrutinee_ty.to_string()
                            )));
                        }
                    };

                    if sub_patterns.len() != element_types.len() {
                        return Err(SemanticError::Other(format!(
                            "Tuple pattern: expected {} elements, found {} patterns",
                            element_types.len(),
                            sub_patterns.len()
                        )));
                    }

                    // Bind the named pattern variable (if any)
                    if arm.pattern.name.is_some() {
                        self.builder.dup();
                        self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;
                    }

                    let tuple_index_id = self.add_string_constant("tuple.index");

                    // Extract and bind each element
                    for (idx, sub_pat) in sub_patterns.iter().enumerate() {
                        if !sub_pat.is_wildcard()
                            && let Pattern::Identifier(id) = sub_pat
                        {
                            self.builder.dup();
                            self.builder.ldi(idx as i64);
                            self.builder.call_host_function(tuple_index_id);
                            let elem_ty = element_types[idx].clone();
                            self.bind_pattern_variable(&Some(id.clone()), elem_ty)?;
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
                    // Wildcard pattern - matches everything (irrefutable)
                    self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                    if !is_last_arm {
                        let end_jump = self.builder.next_address();
                        self.builder.jmp(0);
                        end_jumps.push(end_jump);
                    }
                }

                Pattern::Identifier(id) => {
                    // Bare identifier binding pattern (spec §9.6.1.1):
                    // matches any value and binds `id` to the matched value.
                    // Irrefutable.
                    if arm.pattern.name.is_some() {
                        self.builder.dup();
                        self.bind_pattern_variable(
                            &arm.pattern.name,
                            scrutinee_ty.clone(),
                        )?;
                    }
                    self.bind_pattern_variable(&Some(id.clone()), scrutinee_ty.clone())?;

                    let arm_ty = self.generate_expression(arm.expression.clone())?;
                    self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                    if !is_last_arm {
                        let end_jump = self.builder.next_address();
                        self.builder.jmp(0);
                        end_jumps.push(end_jump);
                    }
                }

                _ => {
                    if let Pattern::Or { alternatives } = pattern {
                        // v1 restriction: alternatives must be literal patterns
                        // or unit-variant `EnumName.Variant` paths. No bindings,
                        // no destructurings, no nested or-patterns.
                        if alternatives.len() < 2 {
                            return Err(SemanticError::Other(
                                "or-pattern must have at least two alternatives".to_string(),
                            ));
                        }
                        for alt in alternatives {
                            match alt {
                                Pattern::IntegerLiteral(_)
                                | Pattern::FloatLiteral(_)
                                | Pattern::StringLiteral(_)
                                | Pattern::BooleanLiteral(_)
                                | Pattern::FieldAccess { .. } => {}
                                Pattern::Identifier(id) => {
                                    return Err(SemanticError::Other(format!(
                                        "or-pattern alternatives must not introduce bindings (got `{}`)",
                                        id.name
                                    )));
                                }
                                _ => {
                                    return Err(SemanticError::Other(
                                        "or-pattern alternatives must be literal or unit-variant patterns in v1"
                                            .to_string(),
                                    ));
                                }
                            }
                        }

                        let is_mixed_enum = matches!(&scrutinee_ty, Type::Enum(id) if {
                            let def = self.symbol_table.get_type(*id);
                            if let TypeDefinition::Enum(e) = def {
                                e.variants.iter().any(|(_, fields)| !fields.is_empty())
                            } else {
                                false
                            }
                        });

                        // Per alternative: compare, jump to body if matched.
                        let mut matched_jumps = Vec::new();
                        for alt in alternatives {
                            self.builder.dup();
                            if is_mixed_enum {
                                self.builder.ldfield(0);
                            }
                            let pattern_expr = self.pattern_to_expression(alt)?;
                            self.generate_expression(pattern_expr)?;
                            self.builder.eq();
                            let j = self.builder.next_address();
                            self.builder.jif(0); // jif jumps on non-zero → matched
                            matched_jumps.push(j);
                        }

                        // None matched: fall through to next arm.
                        let no_match_jump_placeholder = self.builder.next_address();
                        self.builder.jmp(0);

                        // Body label: patch all matched jumps here.
                        let body_address = self.builder.next_address();
                        for j in matched_jumps {
                            self.builder.patch_jump_address(j, body_address);
                        }

                        self.bind_pattern_variable(
                            &arm.pattern.name,
                            scrutinee_ty.clone(),
                        )?;

                        let arm_ty = self.generate_expression(arm.expression.clone())?;
                        self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                        if !is_last_arm {
                            let end_jump_address = self.builder.next_address();
                            self.builder.jmp(0);
                            end_jumps.push(end_jump_address);
                        }

                        let next_arm_address = self.builder.next_address();
                        self.builder.patch_jump_address(
                            no_match_jump_placeholder,
                            next_arm_address,
                        );
                        continue;
                    }

                    // Non-wildcard, non-destructuring: compare with eq.
                    // For enum types that have payload variants, the scrutinee
                    // may be either Int (no-payload) or StructRef (payload).
                    // Extract the tag via ldfield(0) so comparison always
                    // operates on Int vs Int.
                    let is_mixed_enum = matches!(&scrutinee_ty, Type::Enum(id) if {
                        let def = self.symbol_table.get_type(*id);
                        if let TypeDefinition::Enum(e) = def {
                            e.variants.iter().any(|(_, fields)| !fields.is_empty())
                        } else {
                            false
                        }
                    });

                    self.builder.dup();

                    if is_mixed_enum {
                        // Extract tag: Int stays Int, StructRef yields field 0
                        self.builder.ldfield(0);
                    }

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

    /// Spec §9.6.4: every `match` must be exhaustive.
    ///
    /// Rules:
    /// - Any irrefutable arm (wildcard `_`, bare identifier binding,
    ///   tuple pattern, struct pattern) makes the match exhaustive.
    /// - For `bool` scrutinees, covering both `true` and `false` literal
    ///   patterns is exhaustive.
    /// - For `enum` scrutinees, listing every variant (with any sub-patterns)
    ///   is exhaustive.
    /// - For all other types (`int`, `float`, `string`, `char`, `?T`, ...),
    ///   only an irrefutable arm makes the match exhaustive.
    fn check_match_exhaustive(
        &self,
        arms: &[MatchArm],
        scrutinee_ty: &Type,
    ) -> SaResult<()> {
        // Try-else handler arms re-use this codegen path with an exception
        // type as scrutinee and an empty arm list. Treat empty as already
        // exhaustive (try-else has its own handling).
        if arms.is_empty() {
            return Ok(());
        }

        // Any irrefutable arm anywhere makes the match exhaustive.
        if arms.iter().any(|a| Self::pattern_is_irrefutable(&a.pattern.pattern)) {
            return Ok(());
        }

        match scrutinee_ty {
            Type::Primitive(PrimitiveType::Bool) => {
                let mut has_true = false;
                let mut has_false = false;
                for a in arms {
                    Self::for_each_atomic_pattern(&a.pattern.pattern, &mut |p| {
                        if let Pattern::BooleanLiteral(v) = p {
                            if *v { has_true = true; } else { has_false = true; }
                        }
                    });
                }
                if has_true && has_false {
                    return Ok(());
                }
                let missing = match (has_true, has_false) {
                    (false, false) => "`true` and `false`".to_string(),
                    (false, true) => "`true`".to_string(),
                    (true, false) => "`false`".to_string(),
                    (true, true) => unreachable!(),
                };
                Err(SemanticError::NonExhaustiveMatch {
                    scrutinee_ty: "bool".to_string(),
                    missing,
                    pos: None,
                })
            }
            Type::Enum(type_id) => {
                let enum_def = match self.symbol_table.get_type(*type_id) {
                    TypeDefinition::Enum(e) => e.clone(),
                    _ => return Ok(()),
                };
                let mut covered: Vec<bool> = vec![false; enum_def.variants.len()];
                for a in arms {
                    Self::for_each_atomic_pattern(&a.pattern.pattern, &mut |p| {
                        if let Some(variant_name) = Self::enum_variant_name_from_pattern(p) {
                            if let Some((idx, _)) = enum_def
                                .variants
                                .iter()
                                .enumerate()
                                .find(|(_, (n, _))| n.as_ref() == variant_name)
                            {
                                covered[idx] = true;
                            }
                        }
                    });
                }
                let missing: Vec<String> = enum_def
                    .variants
                    .iter()
                    .zip(covered.iter())
                    .filter(|(_, c)| !**c)
                    .map(|((n, _), _)| format!("`{}.{}`", enum_def.qualified_name, n))
                    .collect();
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(SemanticError::NonExhaustiveMatch {
                        scrutinee_ty: enum_def.qualified_name.to_string(),
                        missing: format!("variant(s) {}", missing.join(", ")),
                        pos: None,
                    })
                }
            }
            _ => {
                let ty_str = scrutinee_ty.to_string();
                Err(SemanticError::NonExhaustiveMatch {
                    scrutinee_ty: ty_str,
                    missing: "values outside the listed literals".to_string(),
                    pos: None,
                })
            }
        }
    }

    /// A pattern is irrefutable if it always matches its scrutinee type
    /// (spec §9.6.1.2). For exhaustiveness we treat tuple- and struct-
    /// patterns as irrefutable when present in any arm — codegen and
    /// `pattern_typing` enforce the deeper invariants.
    fn pattern_is_irrefutable(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier(_) => true, // covers both `_` and bare bindings
            Pattern::TuplePattern { sub_patterns } => sub_patterns
                .iter()
                .all(Self::pattern_is_irrefutable),
            Pattern::StructPattern { field_patterns, .. } => field_patterns
                .iter()
                .all(Self::pattern_is_irrefutable),
            _ => false,
        }
    }

    /// Extract the variant name from an enum-shaped pattern, in either
    /// the `EnumName.Variant(...)` form or the bare `EnumName.Variant`
    /// (FieldAccess) unit-variant form.
    fn enum_variant_name_from_pattern(pattern: &Pattern) -> Option<&str> {
        match pattern {
            Pattern::EnumVariant { variant_name, .. } => Some(variant_name.name.as_ref()),
            Pattern::FieldAccess { field, .. } => Some(field.name.as_ref()),
            _ => None,
        }
    }

    /// Walk a pattern, invoking `f` on each leaf alternative — flattening
    /// `Pattern::Or` so callers can treat each branch uniformly.
    fn for_each_atomic_pattern<F: FnMut(&Pattern)>(pattern: &Pattern, f: &mut F) {
        match pattern {
            Pattern::Or { alternatives } => {
                for alt in alternatives {
                    Self::for_each_atomic_pattern(alt, f);
                }
            }
            other => f(other),
        }
    }
}
