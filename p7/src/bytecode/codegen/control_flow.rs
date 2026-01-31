use crate::{
    ast::Expression,
    bytecode::codegen::SaResult,
    errors::SemanticError,
    semantic::{PrimitiveType, Type},
};

use super::Generator;

#[derive(Clone)]
pub(crate) struct LoopContext {
    pub(crate) break_patches: Vec<u32>, // Addresses of break jumps to patch
    pub(crate) continue_target: u32,    // Address to jump to for continue
}

impl Generator {
    pub(super) fn generate_loop(&mut self, body: Expression) -> SaResult<Type> {
        let loop_start = self.builder.next_address();

        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_target: loop_start,
        });

        self.generate_expression(body)?;

        self.builder.jmp(loop_start);

        self.finalize_loop_context();

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_while(
        &mut self,
        condition: Expression,
        body: Expression,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let loop_start = self.builder.next_address();

        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_target: loop_start,
        });

        let condition_type = self.generate_expression(condition)?;
        self.expect_bool_type(&condition_type, pos.0, pos.1)?;

        self.builder.not();
        let exit_jump_placeholder = self.builder.next_address();
        self.builder.jif(0);

        self.generate_expression(body)?;

        self.builder.jmp(loop_start);

        let loop_end = self.builder.next_address();
        self.builder
            .patch_jump_address(exit_jump_placeholder, loop_end);

        self.finalize_loop_context();

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_break(
        &mut self,
        value: Option<Box<Expression>>,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        if value.is_some() {
            return Err(SemanticError::Other(
                "break with value is not yet supported".to_string(),
            ));
        }

        if self.loop_context_stack.is_empty() {
            return Err(SemanticError::Other(format!(
                "break statement outside of loop at line {} column {}",
                pos.0, pos.1
            )));
        }

        let break_jump_addr = self.builder.next_address();
        self.builder.jmp(0);

        if let Some(ctx) = self.loop_context_stack.last_mut() {
            ctx.break_patches.push(break_jump_addr);
        }

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_continue(&mut self, pos: (usize, usize)) -> SaResult<Type> {
        let continue_target = if let Some(ctx) = self.loop_context_stack.last() {
            ctx.continue_target
        } else {
            return Err(SemanticError::Other(format!(
                "continue statement outside of loop at line {} column {}",
                pos.0, pos.1
            )));
        };

        self.builder.jmp(continue_target);

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    /// Patches break statements and cleans up loop context
    fn finalize_loop_context(&mut self) {
        let loop_end = self.builder.next_address();
        if let Some(ctx) = self.loop_context_stack.pop() {
            for break_addr in &ctx.break_patches {
                self.builder.patch_jump_address(*break_addr, loop_end);
            }
        }
    }
}
