use crate::{
    ast::{Expression, FunctionCall, Identifier, Statement},
    bytecode::codegen::SaResult,
    errors::SemanticError,
    intern::InternedString,
    lexer::{Token, TokenType},
    semantic::{PrimitiveType, Type},
};

use super::Generator;

#[derive(Clone)]
pub(crate) struct LoopContext {
    pub(crate) break_patches: Vec<u32>, // Addresses of break jumps to patch
    /// Pending `continue` jumps. Every `continue;` emits a placeholder
    /// `jmp(0)` and records the patch address here. Each loop generator is
    /// responsible for calling `finalize_continue_patches_to(target)` with
    /// its chosen continue target (e.g. `loop_start` for `loop`/`while`, or
    /// the increment block for `for-in`) before `finalize_loop_context`.
    pub(crate) continue_patches: Vec<u32>,
}

impl Generator {
    pub(super) fn generate_loop(&mut self, body: Expression) -> SaResult<Type> {
        let loop_start = self.builder.next_address();

        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_patches: Vec::new(),
        });

        self.generate_expression(body)?;

        self.finalize_continue_patches_to(loop_start);
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
            continue_patches: Vec::new(),
        });

        let condition_type = self.generate_expression(condition)?;
        self.expect_bool_type(&condition_type, pos.0, pos.1)?;

        self.builder.not();
        let exit_jump_placeholder = self.builder.next_address();
        self.builder.jif(0);

        self.generate_expression(body)?;

        self.finalize_continue_patches_to(loop_start);
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
        if self.loop_context_stack.is_empty() {
            return Err(SemanticError::Other(format!(
                "continue statement outside of loop at line {} column {}",
                pos.0, pos.1
            )));
        }

        let patch_addr = self.builder.next_address();
        self.builder.jmp(0);
        self.loop_context_stack
            .last_mut()
            .unwrap()
            .continue_patches
            .push(patch_addr);

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    /// Drain pending `continue` patches on the top `LoopContext` and point
    /// them at `target`. Every loop generator must call this once, with its
    /// chosen continue target, before `finalize_loop_context`.
    fn finalize_continue_patches_to(&mut self, target: u32) {
        let patches =
            std::mem::take(&mut self.loop_context_stack.last_mut().unwrap().continue_patches);
        for addr in patches {
            self.builder.patch_jump_address(addr, target);
        }
    }

    /// Patches break statements and cleans up loop context. The caller is
    /// expected to have already drained `continue_patches` via
    /// `finalize_continue_patches_to`; any leftovers indicate a codegen bug.
    fn finalize_loop_context(&mut self) {
        let loop_end = self.builder.next_address();
        if let Some(ctx) = self.loop_context_stack.pop() {
            for break_addr in &ctx.break_patches {
                self.builder.patch_jump_address(*break_addr, loop_end);
            }
            debug_assert!(
                ctx.continue_patches.is_empty(),
                "unresolved continue patches at loop finalization"
            );
        }
    }

    /// Generate a `for x in arr { ... }` or `for i, x in arr { ... }` loop.
    ///
    /// Semantics (locked in by spec; see specs/protosept-language.md):
    /// - The iterable is evaluated exactly once and stored in a hidden local.
    /// - `arr.len()` is snapshotted once; mid-loop pushes/pops are not visited.
    /// - The value binding `x` is by-value when the element type is
    ///   Copy-treated, otherwise it is automatically bound as `ref<T>` so that
    ///   the body can call methods / read fields without copying.
    /// - The optional `index_var` is bound to the 0-based iteration counter.
    /// - `break` jumps past the loop; `continue` jumps to the increment block
    ///   (not the condition check) so that the index counter still advances.
    /// - The iterable is bound via direct codegen rather than `Statement::Let`,
    ///   so it does NOT trigger move-tracking on the source variable; the
    ///   caller can still reference the original array after the loop.
    pub(super) fn generate_for_in(
        &mut self,
        index_var: Option<Identifier>,
        value_var: Identifier,
        iterable: Expression,
        body: Expression,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let n = self.for_in_counter;
        self.for_in_counter = self.for_in_counter.wrapping_add(1);

        let arr_name: InternedString = format!("$for_arr_{}", n).into();
        let len_name: InternedString = format!("$for_len_{}", n).into();
        let i_name: InternedString = format!("$for_i_{}", n).into();

        let synth_ident = |name: InternedString| Identifier {
            name,
            line: pos.0,
            col: pos.1,
        };
        let synth_token = |tt: TokenType| Token {
            token_type: tt,
            line: pos.0,
            col: pos.1,
            length: 0,
        };
        let ident_expr = |name: InternedString| Expression::Identifier(synth_ident(name));

        self.local_scope.as_mut().unwrap().push_scope();
        let result = (|| -> SaResult<Type> {
            // Evaluate the iterable directly (bypass Statement::Let so no
            // move-tracking is recorded against the source variable).
            let arr_ty = self.generate_expression(iterable)?;

            // If the iterable evaluated to a raw `array<T>`, retag the hidden
            // local as `ref<array<T>>` so subsequent reads in the loop body
            // (via Identifier load) do not look like move candidates. This is
            // purely a type-level retag — the runtime representation of `ref`
            // is identical to the underlying value (see `generate_ref`).
            let stored_arr_ty = match &arr_ty {
                Type::Array(_) => Type::Reference(Box::new(arr_ty.clone())),
                _ => arr_ty.clone(),
            };

            let arr_var_id = self
                .local_scope
                .as_mut()
                .unwrap()
                .add_variable(arr_name.clone(), stored_arr_ty, false)
                .map_err(|_| SemanticError::Other("failed to bind $for_arr".to_string()))?;
            self.builder.stvar(arr_var_id);

            // Re-fetch the stored type for downstream decisions.
            let stored_ty = self
                .local_scope
                .as_ref()
                .unwrap()
                .get_variable_type(arr_var_id);

            if let Some(elem_ty) = unwrap_array_type(&stored_ty) {
                // ----- Array fast path -----
                self.generate_for_in_array_fast_path(
                    arr_name, arr_var_id, len_name, i_name, elem_ty, index_var, value_var, body,
                    pos,
                )?;
            } else {
                // ----- Iterable proto path -----
                self.generate_for_in_iterable_proto_path(
                    arr_name, n, index_var, value_var, body, pos,
                )?;
            }

            // Suppress unused-token warning if a path didn't use synth_token.
            let _ = synth_token;

            Ok(Type::Primitive(PrimitiveType::Unit))
        })();
        self.local_scope.as_mut().unwrap().pop_scope();
        result
    }

    /// Array fast path: snapshot length, indexed loop, auto-`ref<T>` on
    /// non-Copy element types. Matches the semantics documented in
    /// `specs/protosept-language.md` §9.5.1.
    #[allow(clippy::too_many_arguments)]
    fn generate_for_in_array_fast_path(
        &mut self,
        arr_name: InternedString,
        _arr_var_id: u32,
        len_name: InternedString,
        i_name: InternedString,
        elem_ty: Type,
        index_var: Option<Identifier>,
        value_var: Identifier,
        body: Expression,
        pos: (usize, usize),
    ) -> SaResult<()> {
        let synth_ident = |name: InternedString| Identifier {
            name,
            line: pos.0,
            col: pos.1,
        };
        let ident_expr = |name: InternedString| Expression::Identifier(synth_ident(name));
        let elem_is_copy = elem_ty.is_copy_treated(&self.symbol_table);

        // let $for_len_N = $for_arr_N.len();
        let len_call = Expression::FunctionCall(FunctionCall {
            callee: Box::new(Expression::FieldAccess {
                object: Box::new(ident_expr(arr_name.clone())),
                field: synth_ident(InternedString::from("len")),
            }),
            arguments: Vec::new(),
        });
        self.generate_expression(len_call)?;
        let len_var_id = self
            .local_scope
            .as_mut()
            .unwrap()
            .add_variable(len_name.clone(), Type::Primitive(PrimitiveType::Int), false)
            .map_err(|_| SemanticError::Other("failed to bind $for_len".to_string()))?;
        self.builder.stvar(len_var_id);

        // let mut $for_i_N = 0;
        self.builder.ldi(0);
        let i_var_id = self
            .local_scope
            .as_mut()
            .unwrap()
            .add_variable(i_name.clone(), Type::Primitive(PrimitiveType::Int), true)
            .map_err(|_| SemanticError::Other("failed to bind $for_i".to_string()))?;
        self.builder.stvar(i_var_id);

        let loop_start = self.builder.next_address();
        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_patches: Vec::new(),
        });

        // Condition
        self.builder.ldvar(i_var_id);
        self.builder.ldvar(len_var_id);
        self.builder.lt();
        self.builder.not();
        let exit_jump_placeholder = self.builder.next_address();
        self.builder.jif(0);

        let arr_index = Expression::ArrayIndex {
            array: Box::new(ident_expr(arr_name.clone())),
            index: Box::new(ident_expr(i_name.clone())),
            pos,
        };
        let value_init = if elem_is_copy {
            arr_index
        } else {
            Expression::Ref(Box::new(arr_index))
        };

        let mut body_stmts: Vec<Statement> = Vec::new();
        if let Some(idx_ident) = index_var {
            body_stmts.push(Statement::Let {
                is_pub: false,
                is_mutable: false,
                identifier: idx_ident,
                type_annotation: None,
                expression: ident_expr(i_name.clone()),
            });
        }
        body_stmts.push(Statement::Let {
            is_pub: false,
            is_mutable: false,
            identifier: value_var,
            type_annotation: None,
            expression: value_init,
        });
        body_stmts.push(Statement::ExpressionStatement(body));

        self.generate_expression(Expression::Block(body_stmts))?;

        // Increment block — the continue target.
        let inc_addr = self.builder.next_address();
        self.finalize_continue_patches_to(inc_addr);

        self.builder.ldvar(i_var_id);
        self.builder.ldi(1);
        self.builder.addi();
        self.builder.stvar(i_var_id);

        self.builder.jmp(loop_start);

        let loop_end = self.builder.next_address();
        self.builder
            .patch_jump_address(exit_jump_placeholder, loop_end);

        self.finalize_loop_context();
        Ok(())
    }

    /// Iterable proto path: dispatch through `iter()` / `next()` methods.
    /// Verified structurally (no generic protos in p7 today; see
    /// `specs/protosept-language.md` §9.5.1 — "the protos are non-generic
    /// markers").
    ///
    /// Generates:
    /// ```text
    /// {
    ///     let $for_iter_N = $for_arr_N.iter();    // box<SomeIter>
    ///     let mut $for_idx_N = 0;                  // optional, indexed form only
    ///     loop {
    ///         let $for_cur_N = $for_iter_N.next();  // ?T
    ///         if $for_cur_N == null { break; }
    ///         let i = $for_idx_N;                   // optional
    ///         let x = $for_cur_N!;                  // unwrap ?T -> T
    ///         <body>
    ///         $for_idx_N = $for_idx_N + 1;          // optional
    ///     }
    /// }
    /// ```
    /// `continue` from `<body>` advances the index counter (when present) but
    /// jumps to the next `next()` call.
    fn generate_for_in_iterable_proto_path(
        &mut self,
        arr_name: InternedString,
        n: u32,
        index_var: Option<Identifier>,
        value_var: Identifier,
        body: Expression,
        pos: (usize, usize),
    ) -> SaResult<()> {
        let synth_ident = |name: InternedString| Identifier {
            name,
            line: pos.0,
            col: pos.1,
        };
        let synth_token = |tt: TokenType| Token {
            token_type: tt,
            line: pos.0,
            col: pos.1,
            length: 0,
        };
        let ident_expr = |name: InternedString| Expression::Identifier(synth_ident(name));

        let iter_name: InternedString = format!("$for_iter_{}", n).into();
        let cur_name: InternedString = format!("$for_cur_{}", n).into();
        let idx_name: InternedString = format!("$for_idx_{}", n).into();

        // let $for_iter_N = $for_arr_N.iter();
        let iter_call = Expression::FunctionCall(FunctionCall {
            callee: Box::new(Expression::FieldAccess {
                object: Box::new(ident_expr(arr_name.clone())),
                field: synth_ident(InternedString::from("iter")),
            }),
            arguments: Vec::new(),
        });
        self.generate_statement(Statement::Let {
            is_pub: false,
            is_mutable: false,
            identifier: synth_ident(iter_name.clone()),
            type_annotation: None,
            expression: iter_call,
        })?;

        // Optional `let mut $for_idx_N = 0;` for the indexed form.
        let has_index = index_var.is_some();
        if has_index {
            self.generate_statement(Statement::Let {
                is_pub: false,
                is_mutable: true,
                identifier: synth_ident(idx_name.clone()),
                type_annotation: None,
                expression: Expression::IntegerLiteral(0),
            })?;
        }

        // Build the loop body. We use a `loop { ... }` (infinite) and exit
        // via `break` when `next()` returns null.
        //
        //   let $for_cur_N = $for_iter_N.next();
        //   if $for_cur_N == null { break; }
        //   let i = $for_idx_N;     (optional)
        //   let x = $for_cur_N!;
        //   <body>
        //   $for_idx_N = $for_idx_N + 1;   (optional)
        let next_call = Expression::FunctionCall(FunctionCall {
            callee: Box::new(Expression::FieldAccess {
                object: Box::new(ident_expr(iter_name.clone())),
                field: synth_ident(InternedString::from("next")),
            }),
            arguments: Vec::new(),
        });
        let cur_is_null = Expression::Binary {
            left: Box::new(ident_expr(cur_name.clone())),
            operator: synth_token(TokenType::Equals),
            right: Box::new(Expression::NullLiteral),
        };
        let break_block =
            Expression::Block(vec![Statement::ExpressionStatement(Expression::Break {
                value: None,
                pos,
            })]);
        let null_check = Expression::If {
            condition: Box::new(cur_is_null),
            then_branch: Box::new(break_block),
            else_branch: None,
            pos,
        };
        let unwrap_cur = Expression::ForceUnwrap {
            operand: Box::new(ident_expr(cur_name.clone())),
            token: synth_token(TokenType::Exclamation),
        };

        let mut body_stmts: Vec<Statement> = Vec::new();
        body_stmts.push(Statement::Let {
            is_pub: false,
            is_mutable: false,
            identifier: synth_ident(cur_name.clone()),
            type_annotation: None,
            expression: next_call,
        });
        body_stmts.push(Statement::ExpressionStatement(null_check));
        if let Some(idx_ident) = index_var {
            body_stmts.push(Statement::Let {
                is_pub: false,
                is_mutable: false,
                identifier: idx_ident,
                type_annotation: None,
                expression: ident_expr(idx_name.clone()),
            });
        }
        body_stmts.push(Statement::Let {
            is_pub: false,
            is_mutable: false,
            identifier: value_var,
            type_annotation: None,
            expression: unwrap_cur,
        });
        body_stmts.push(Statement::ExpressionStatement(body));

        // We need `continue` to advance the index counter (if present) and
        // then loop back to the `next()` call. Since `loop`/`while` both
        // route `continue` to `loop_start`, the increment must come BEFORE
        // we resume the next iteration. Emit the loop manually so the
        // increment block is the continue target.

        // Emit `loop_start:`, push deferred LoopContext, generate body,
        // patch continue jumps to point at the increment block, then jmp
        // back to loop_start.
        let loop_start = self.builder.next_address();
        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_patches: Vec::new(),
        });

        self.generate_expression(Expression::Block(body_stmts))?;

        // Increment block — continue target.
        let inc_addr = self.builder.next_address();
        self.finalize_continue_patches_to(inc_addr);

        if has_index {
            // $for_idx_N = $for_idx_N + 1
            let idx_var_id = self
                .local_scope
                .as_ref()
                .unwrap()
                .find_variable(&idx_name)
                .expect("$for_idx_N must be in scope");
            self.builder.ldvar(idx_var_id);
            self.builder.ldi(1);
            self.builder.addi();
            self.builder.stvar(idx_var_id);
        }

        self.builder.jmp(loop_start);

        // `break` from the null-check path lands here.
        self.finalize_loop_context();
        Ok(())
    }
}

/// Reduce `array<T>`, `ref<array<T>>`, or `box<array<T>>` to `T`.
fn unwrap_array_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(inner) => Some((**inner).clone()),
        Type::Reference(inner) | Type::MutableReference(inner) | Type::BoxType(inner) => {
            if let Type::Array(elem) = inner.as_ref() {
                Some((**elem).clone())
            } else {
                None
            }
        }
        _ => None,
    }
}
