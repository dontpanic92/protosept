use super::{Generator, SaResult};
use crate::ast::MatchArm;
use crate::semantic::{PrimitiveType, Type};

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

            // Check if this is a wildcard pattern
            if !arm.pattern.pattern.is_wildcard() {
                // Non-wildcard pattern: need to compare
                self.builder.dup();

                // Generate code to load the pattern value
                let pattern_expr = self.pattern_to_expression(&arm.pattern.pattern)?;
                self.generate_expression(pattern_expr)?;

                // Compare: are they equal?
                self.builder.eq();

                // Negate the result: 1 if not equal, 0 if equal
                self.builder.not();

                // If not equal (result is 1 after not), jump to next arm
                let no_match_jump_placeholder = self.builder.next_address();
                self.builder.jif(0); // Placeholder

                // Pattern matched! Bind to variable if there's a name
                self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                // Generate the expression for this arm
                let arm_ty = self.generate_expression(arm.expression.clone())?;
                self.validate_match_arm_type(&mut result_ty, arm_ty)?;

                // Jump to end of all arms (unless this is the last arm)
                if !is_last_arm {
                    let end_jump_address = self.builder.next_address();
                    self.builder.jmp(0); // Placeholder
                    end_jumps.push(end_jump_address);
                }

                // Patch the no-match jump to point here (next arm)
                let next_arm_address = self.builder.next_address();
                self.builder
                    .patch_jump_address(no_match_jump_placeholder, next_arm_address);
            } else {
                // Wildcard pattern - matches everything
                self.bind_pattern_variable(&arm.pattern.name, scrutinee_ty.clone())?;

                // Generate the expression for this arm
                let arm_ty = self.generate_expression(arm.expression.clone())?;
                self.validate_match_arm_type(&mut result_ty, arm_ty)?;
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
