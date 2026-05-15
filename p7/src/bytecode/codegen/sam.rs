//! SAM (Single Abstract Method) coercion for closures (L2).
//!
//! When a closure literal appears at an expected-type site `box<F>` where
//! `F` is an object proto with exactly one abstract method whose signature
//! matches the closure (ignoring the leading `self: ref<F>` parameter),
//! the closure elaborates to an anonymous `struct[F]` with the closure's
//! free variables as fields, and an `impl` method whose body is the
//! closure body. Free-variable references inside the body are rewritten
//! to `self.field` accesses.
//!
//! This keeps the language ABI-blind: the synthesized struct is an
//! ordinary user struct that lists `F` in its conformance bracket, and
//! routes through the same `box<T> -> box<F>` coercion (BoxToProto) that
//! L1 enabled for explicit script-impl-of-foreign-proto structs.
//!
//! Restrictions for v1:
//!
//! - `F` must have exactly one method, and that method's first parameter
//!   must be a `ref<F>` self (the conventional proto method shape).
//! - The closure's parameter list must match the rest of the method
//!   signature (after self), with `types_compatible` semantics.
//! - The body's value type must be compatible with the method's declared
//!   return type (using existing checking-context coercions).
//! - The closure body must not declare its own proto-conforming struct,
//!   nested SAM closures, or otherwise rely on capture rebinding inside
//!   inner closures (existing `Closure` capture rules still apply for
//!   nested ordinary closures, but free-var name shadowing by inner
//!   parameters/lets is not supported).

use std::collections::HashSet;

use crate::ast::{Expression, Identifier, Parameter};
use crate::bytecode::Instruction;
use crate::errors::SemanticError;
use crate::intern::InternedString;
use crate::semantic::{
    Function, LocalSymbolScope, PrimitiveType, Struct, Symbol, SymbolKind, Type, TypeDefinition,
    TypeId, Variable,
};

use super::{Generator, SaResult};

impl Generator {
    /// Attempt to elaborate a closure literal at a `box<P>` expected-type
    /// site into an anonymous-struct-implements-`P` construction.
    ///
    /// Returns `Ok(Some(box<P>))` when the closure was successfully
    /// elaborated (call-site bytecode has been emitted). Returns
    /// `Ok(None)` when the target is not SAM-eligible; callers should
    /// fall through to ordinary closure codegen.
    pub(super) fn try_elaborate_sam_closure(
        &mut self,
        parameters: Vec<Parameter>,
        body: Expression,
        _pos: (usize, usize),
        proto_id: TypeId,
    ) -> SaResult<Option<Type>> {
        // 1. Resolve proto and find single abstract method.
        let (method_name, method_params, method_return_type) = {
            let proto = match self.symbol_table.types.get(proto_id as usize) {
                Some(TypeDefinition::Proto(p)) => p,
                _ => return Ok(None),
            };
            if proto.methods.len() != 1 {
                return Ok(None);
            }
            let (name, params, ret_ty) = &proto.methods[0];
            (name.clone(), params.clone(), ret_ty.clone())
        };

        // Method must have a leading `self: ref<P>` (the convention used
        // by every proto we synthesize for; e.g. IAction.invoke).
        if method_params.is_empty() {
            return Ok(None);
        }
        match &method_params[0] {
            Type::Reference(inner) => match inner.as_ref() {
                Type::Proto(pid) if *pid == proto_id => {}
                _ => return Ok(None),
            },
            _ => return Ok(None),
        }

        let expected_closure_param_count = method_params.len() - 1;
        if parameters.len() != expected_closure_param_count {
            return Ok(None);
        }

        // 2. Resolve closure parameter types and verify compatibility.
        let closure_params: Vec<Variable> = parameters
            .iter()
            .map(|p| {
                self.get_semantic_type(&p.arg_type).map(|ty| Variable {
                    name: p.name.name.clone(),
                    ty,
                    is_mutable: false,
                })
            })
            .collect::<SaResult<Vec<_>>>()?;

        for (i, var) in closure_params.iter().enumerate() {
            let expected = &method_params[i + 1];
            if !self.types_compatible(&var.ty, expected) {
                return Ok(None);
            }
        }

        let method_return_resolved = method_return_type
            .clone()
            .unwrap_or(Type::Primitive(PrimitiveType::Unit));

        // 3. Collect free variables (captures) referenced by the body.
        let referenced = collect_identifiers(&body);
        let param_names: HashSet<InternedString> =
            closure_params.iter().map(|v| v.name.clone()).collect();

        let mut free_vars: Vec<(InternedString, Type, bool)> = Vec::new();
        let mut seen: HashSet<InternedString> = HashSet::new();
        if let Some(scope) = &self.local_scope {
            for name in &referenced {
                if param_names.contains(name) || seen.contains(name) {
                    continue;
                }
                if let Some(var_id) = scope.find_variable(name) {
                    let ty = scope.get_variable_type(var_id);
                    free_vars.push((name.clone(), ty, false));
                    seen.insert(name.clone());
                } else if let Some(param_id) = scope.find_param(name) {
                    let ty = scope.get_param_type(param_id);
                    free_vars.push((name.clone(), ty, true));
                    seen.insert(name.clone());
                }
            }
        }

        // Reject captures that hold non-escapable ref types — these can't
        // become struct fields (same restriction as ordinary structs).
        for (name, ty, _) in &free_vars {
            if matches!(ty, Type::Reference(_) | Type::MutableReference(_)) {
                return Err(SemanticError::Other(format!(
                    "SAM-coerced closure cannot capture ref-typed variable '{}'",
                    name
                )));
            }
        }

        // 4. Synthesize anonymous struct type listing F in its
        //    conformance bracket, with captures as fields.
        let counter = self.sam_anon_counter;
        self.sam_anon_counter += 1;
        let local_name = InternedString::from(format!("__anon_closure_{}", counter));
        let struct_qualified_name =
            self.symbol_table.get_new_symbol_qualified_name(&local_name);

        let fields: Vec<(InternedString, Type)> = free_vars
            .iter()
            .map(|(n, t, _)| (n.clone(), t.clone()))
            .collect();
        let field_visibility: Vec<bool> = vec![false; free_vars.len()];

        let struct_def = Struct {
            qualified_name: struct_qualified_name.clone(),
            is_pub: false,
            fields,
            field_visibility,
            field_defaults: vec![None; free_vars.len()],
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            type_param_bounds: Vec::new(),
            generic_field_types: None,
            monomorphization: None,
            conforming_to: vec![proto_id],
            methods: Vec::new(),
            source_module: None,
        };
        let struct_type_id = self.symbol_table.add_type(TypeDefinition::Struct(struct_def));

        // Register the struct symbol as a child of whatever is current in
        // the symbol chain. `build_vtable` iterates `module.symbols`
        // flatly, so the parent location does not matter for dispatch.
        let struct_symbol = Symbol::new(
            local_name.clone(),
            struct_qualified_name.clone(),
            SymbolKind::Type(struct_type_id),
        );
        self.symbol_table.push_symbol(struct_symbol);

        // 5. Emit a jmp around the method body so the surrounding
        //    function's bytecode continues past it.
        let jmp_placeholder = self.builder.next_address();
        self.builder.jmp(0);
        let method_body_addr = self.builder.next_address();

        // 6. Synthesize the proto method on the struct.
        let method_qualified_name = InternedString::from(format!(
            "{}.{}",
            struct_qualified_name, method_name
        ));

        let self_var = Variable {
            name: InternedString::from("self"),
            ty: Type::Reference(Box::new(Type::Struct(struct_type_id))),
            is_mutable: false,
        };
        let mut method_param_vars: Vec<Variable> = Vec::with_capacity(closure_params.len() + 1);
        method_param_vars.push(self_var);
        method_param_vars.extend(closure_params.iter().cloned());

        let func = Function {
            qualified_name: method_qualified_name.clone(),
            is_pub: true,
            params: method_param_vars.iter().map(|v| v.ty.clone()).collect(),
            param_names: method_param_vars.iter().map(|v| v.name.clone()).collect(),
            param_defaults: vec![None; method_param_vars.len()],
            return_type: method_return_resolved.clone(),
            attributes: Vec::new(),
            intrinsic_name: None,
            type_parameters: Vec::new(),
            type_param_bounds: Vec::new(),
            generic_param_types: None,
            generic_return_type: None,
            generic_body: None,
            monomorphization: None,
        };
        let func_id = self.symbol_table.add_function(func);

        let method_symbol = Symbol::new(
            method_name.clone(),
            method_qualified_name,
            SymbolKind::Function {
                func_id,
                address: method_body_addr,
            },
        );
        self.symbol_table.push_symbol(method_symbol);

        // 7. Save caller state and switch to method-body codegen state.
        let saved_local_scope = self.local_scope.take();
        let saved_self_type = self.current_self_type.take();
        let saved_moved_vars = std::mem::take(&mut self.moved_variables);
        let saved_moved_params = std::mem::take(&mut self.moved_params);
        let saved_loop_ctx = std::mem::take(&mut self.loop_context_stack);

        self.local_scope = Some(LocalSymbolScope::new(method_param_vars.clone()));
        self.current_self_type = Some(Type::Struct(struct_type_id));

        // Rewrite captures in body to access them as `self.field`.
        let capture_names: HashSet<InternedString> =
            free_vars.iter().map(|(n, _, _)| n.clone()).collect();
        let rewritten_body = rewrite_captures(body, &capture_names);

        self.enclosing_return_types.push(method_return_resolved.clone());
        let body_codegen_result = self.generate_expression_with_expected_type(
            rewritten_body,
            Some(&method_return_resolved),
        );
        self.enclosing_return_types.pop();

        // Always restore state, even on body codegen error, before
        // returning the error.
        self.local_scope = saved_local_scope;
        self.current_self_type = saved_self_type;
        self.moved_variables = saved_moved_vars;
        self.moved_params = saved_moved_params;
        self.loop_context_stack = saved_loop_ctx;

        let body_type = body_codegen_result?;
        if !self.types_compatible(&body_type, &method_return_resolved) {
            // Pop the method + struct symbols before erroring out so the
            // symbol chain stays balanced.
            self.symbol_table.pop_symbol();
            self.symbol_table.pop_symbol();
            return Err(SemanticError::TypeMismatch {
                lhs: format!(
                    "SAM closure body returns {}",
                    self.type_to_string(&body_type)
                ),
                rhs: format!(
                    "proto method '{}' declares return type {}",
                    method_name,
                    self.type_to_string(&method_return_resolved)
                ),
                pos: None,
            });
        }

        self.builder.ret();

        // Pop method symbol and struct symbol from the chain.
        self.symbol_table.pop_symbol();
        self.symbol_table.pop_symbol();

        // Patch the skip-over jmp.
        let after_method = self.builder.next_address();
        self.builder.patch_jump_address(jmp_placeholder, after_method);

        // 8. Emit the call-site code: push captures, NewStruct, BoxAlloc,
        //    BoxToProto. (No conditional GC barrier: BoxAlloc handles
        //    that internally.)
        for (name, _ty, is_param) in &free_vars {
            if let Some(scope) = &self.local_scope {
                if *is_param {
                    if let Some(param_id) = scope.find_param(name) {
                        self.builder.ldpar(param_id);
                    }
                } else if let Some(var_id) = scope.find_variable(name) {
                    self.builder.ldvar(var_id);
                }
            }
        }
        self.builder.newstruct(free_vars.len() as u32);
        self.builder.box_alloc();
        self.builder
            .add_instruction(Instruction::BoxToProto(struct_type_id, proto_id));

        Ok(Some(Type::BoxType(Box::new(Type::Proto(proto_id)))))
    }
}

/// Recursively collect identifier names referenced in an expression.
/// Mirrors `Generator::collect_identifiers` in literals.rs but lives
/// here to keep this module standalone.
fn collect_identifiers(expr: &Expression) -> Vec<InternedString> {
    let mut names = Vec::new();
    collect_identifiers_rec(expr, &mut names);
    names
}

fn collect_identifiers_rec(expr: &Expression, names: &mut Vec<InternedString>) {
    match expr {
        Expression::Identifier(id) => names.push(id.name.clone()),
        Expression::Binary { left, right, .. } => {
            collect_identifiers_rec(left, names);
            collect_identifiers_rec(right, names);
        }
        Expression::Unary { right, .. } => collect_identifiers_rec(right, names),
        Expression::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_identifiers_rec(condition, names);
            collect_identifiers_rec(then_branch, names);
            if let Some(eb) = else_branch {
                collect_identifiers_rec(eb, names);
            }
        }
        Expression::FunctionCall(call) => {
            collect_identifiers_rec(&call.callee, names);
            for (_, arg) in &call.arguments {
                collect_identifiers_rec(arg, names);
            }
        }
        Expression::FieldAccess { object, .. } => collect_identifiers_rec(object, names),
        Expression::Block(stmts) => {
            for stmt in stmts {
                match stmt {
                    crate::ast::Statement::Expression(e) => collect_identifiers_rec(e, names),
                    crate::ast::Statement::Let { expression, .. } => {
                        collect_identifiers_rec(expression, names)
                    }
                    crate::ast::Statement::Return { expression, .. } => {
                        if let Some(e) = expression {
                            collect_identifiers_rec(e, names);
                        }
                    }
                    crate::ast::Statement::Throw(e) => collect_identifiers_rec(e, names),
                    _ => {}
                }
            }
        }
        Expression::ArrayLiteral { elements, .. } => {
            for el in elements {
                collect_identifiers_rec(el, names);
            }
        }
        Expression::ArrayIndex { array, index, .. } => {
            collect_identifiers_rec(array, names);
            collect_identifiers_rec(index, names);
        }
        Expression::ForceUnwrap { operand, .. } => collect_identifiers_rec(operand, names),
        Expression::Ref(inner) | Expression::BlockValue(inner) => {
            collect_identifiers_rec(inner, names)
        }
        Expression::Cast { expression, .. } => collect_identifiers_rec(expression, names),
        Expression::Loop { body, .. } => collect_identifiers_rec(body, names),
        Expression::While {
            condition, body, ..
        } => {
            collect_identifiers_rec(condition, names);
            collect_identifiers_rec(body, names);
        }
        Expression::Closure { body, .. } => collect_identifiers_rec(body, names),
        Expression::Try {
            try_block,
            else_arms,
        } => {
            collect_identifiers_rec(try_block, names);
            for arm in else_arms {
                collect_identifiers_rec(&arm.expression, names);
            }
        }
        Expression::Match { scrutinee, arms } => {
            collect_identifiers_rec(scrutinee, names);
            for arm in arms {
                collect_identifiers_rec(&arm.expression, names);
            }
        }
        Expression::TupleLiteral { elements, .. } => {
            for el in elements {
                collect_identifiers_rec(el, names);
            }
        }
        Expression::StructUpdate { base, updates, .. } => {
            collect_identifiers_rec(base, names);
            for (_, e) in updates {
                collect_identifiers_rec(e, names);
            }
        }
        Expression::MapLiteral { pairs, .. } => {
            for (k, v) in pairs {
                collect_identifiers_rec(k, names);
                collect_identifiers_rec(v, names);
            }
        }
        Expression::Break { value, .. } => {
            if let Some(v) = value {
                collect_identifiers_rec(v, names);
            }
        }
        Expression::InterpolatedString { parts } => {
            for part in parts {
                if let crate::ast::InterpolatedStringPart::Expr(e) = part {
                    collect_identifiers_rec(e, names);
                }
            }
        }
        _ => {}
    }
}

/// Rewrite every expression-position `Identifier(name)` whose `name` is
/// in `captures` to `self.name`. Recurses through composite expressions
/// and statements. Note: this is a deliberately simple structural
/// rewrite — name shadowing introduced inside the body (e.g. via inner
/// `let` or nested closure parameters) is not honored, so SAM-coerced
/// closures must not rebind capture names. Matches the v1 restriction
/// documented in the module preamble.
fn rewrite_captures(expr: Expression, captures: &HashSet<InternedString>) -> Expression {
    match expr {
        Expression::Identifier(id) => {
            if captures.contains(&id.name) {
                let self_id = Identifier {
                    name: InternedString::from("self"),
                    line: id.line,
                    col: id.col,
                };
                Expression::FieldAccess {
                    object: Box::new(Expression::Identifier(self_id)),
                    field: id,
                }
            } else {
                Expression::Identifier(id)
            }
        }
        Expression::Binary {
            left,
            operator,
            right,
        } => Expression::Binary {
            left: Box::new(rewrite_captures(*left, captures)),
            operator,
            right: Box::new(rewrite_captures(*right, captures)),
        },
        Expression::Unary { operator, right } => Expression::Unary {
            operator,
            right: Box::new(rewrite_captures(*right, captures)),
        },
        Expression::If {
            condition,
            then_branch,
            else_branch,
            pos,
        } => Expression::If {
            condition: Box::new(rewrite_captures(*condition, captures)),
            then_branch: Box::new(rewrite_captures(*then_branch, captures)),
            else_branch: else_branch.map(|e| Box::new(rewrite_captures(*e, captures))),
            pos,
        },
        Expression::FunctionCall(call) => {
            let callee = Box::new(rewrite_captures(*call.callee, captures));
            let arguments = call
                .arguments
                .into_iter()
                .map(|(name, e)| (name, rewrite_captures(e, captures)))
                .collect();
            Expression::FunctionCall(crate::ast::FunctionCall { callee, arguments })
        }
        Expression::FieldAccess { object, field } => Expression::FieldAccess {
            object: Box::new(rewrite_captures(*object, captures)),
            field,
        },
        Expression::Block(stmts) => Expression::Block(
            stmts
                .into_iter()
                .map(|s| rewrite_captures_stmt(s, captures))
                .collect(),
        ),
        Expression::Try {
            try_block,
            else_arms,
        } => Expression::Try {
            try_block: Box::new(rewrite_captures(*try_block, captures)),
            else_arms: else_arms
                .into_iter()
                .map(|arm| crate::ast::MatchArm {
                    pattern: arm.pattern,
                    expression: rewrite_captures(arm.expression, captures),
                })
                .collect(),
        },
        Expression::Match { scrutinee, arms } => Expression::Match {
            scrutinee: Box::new(rewrite_captures(*scrutinee, captures)),
            arms: arms
                .into_iter()
                .map(|arm| crate::ast::MatchArm {
                    pattern: arm.pattern,
                    expression: rewrite_captures(arm.expression, captures),
                })
                .collect(),
        },
        Expression::Ref(inner) => Expression::Ref(Box::new(rewrite_captures(*inner, captures))),
        Expression::BlockValue(inner) => {
            Expression::BlockValue(Box::new(rewrite_captures(*inner, captures)))
        }
        Expression::Cast {
            expression,
            target_type,
        } => Expression::Cast {
            expression: Box::new(rewrite_captures(*expression, captures)),
            target_type,
        },
        Expression::Loop { body, pos } => Expression::Loop {
            body: Box::new(rewrite_captures(*body, captures)),
            pos,
        },
        Expression::While {
            condition,
            body,
            pos,
        } => Expression::While {
            condition: Box::new(rewrite_captures(*condition, captures)),
            body: Box::new(rewrite_captures(*body, captures)),
            pos,
        },
        Expression::Break { value, pos } => Expression::Break {
            value: value.map(|v| Box::new(rewrite_captures(*v, captures))),
            pos,
        },
        Expression::ArrayLiteral { elements, pos } => Expression::ArrayLiteral {
            elements: elements
                .into_iter()
                .map(|e| rewrite_captures(e, captures))
                .collect(),
            pos,
        },
        Expression::ArrayIndex { array, index, pos } => Expression::ArrayIndex {
            array: Box::new(rewrite_captures(*array, captures)),
            index: Box::new(rewrite_captures(*index, captures)),
            pos,
        },
        Expression::ForceUnwrap { operand, token } => Expression::ForceUnwrap {
            operand: Box::new(rewrite_captures(*operand, captures)),
            token,
        },
        Expression::Closure {
            parameters,
            body,
            pos,
        } => Expression::Closure {
            parameters,
            body: Box::new(rewrite_captures(*body, captures)),
            pos,
        },
        Expression::TupleLiteral { elements, pos } => Expression::TupleLiteral {
            elements: elements
                .into_iter()
                .map(|e| rewrite_captures(e, captures))
                .collect(),
            pos,
        },
        Expression::StructUpdate {
            struct_name,
            base,
            updates,
            pos,
        } => Expression::StructUpdate {
            struct_name: Box::new(rewrite_captures(*struct_name, captures)),
            base: Box::new(rewrite_captures(*base, captures)),
            updates: updates
                .into_iter()
                .map(|(name, e)| (name, rewrite_captures(e, captures)))
                .collect(),
            pos,
        },
        Expression::MapLiteral { pairs, pos } => Expression::MapLiteral {
            pairs: pairs
                .into_iter()
                .map(|(k, v)| (rewrite_captures(k, captures), rewrite_captures(v, captures)))
                .collect(),
            pos,
        },
        Expression::InterpolatedString { parts } => Expression::InterpolatedString {
            parts: parts
                .into_iter()
                .map(|part| match part {
                    crate::ast::InterpolatedStringPart::Expr(e) => {
                        crate::ast::InterpolatedStringPart::Expr(rewrite_captures(e, captures))
                    }
                    other => other,
                })
                .collect(),
        },
        // Leaves: literals, NullLiteral, Continue, GenericInstantiation
        other => other,
    }
}

fn rewrite_captures_stmt(
    stmt: crate::ast::Statement,
    captures: &HashSet<InternedString>,
) -> crate::ast::Statement {
    use crate::ast::Statement;
    match stmt {
        Statement::Expression(e) => Statement::Expression(rewrite_captures(e, captures)),
        Statement::Let {
            is_pub,
            is_mutable,
            identifier,
            type_annotation,
            expression,
        } => Statement::Let {
            is_pub,
            is_mutable,
            identifier,
            type_annotation,
            expression: rewrite_captures(expression, captures),
        },
        Statement::LetDestructure {
            is_mutable,
            pattern,
            expression,
        } => Statement::LetDestructure {
            is_mutable,
            pattern,
            expression: rewrite_captures(expression, captures),
        },
        Statement::Return { expression, pos } => Statement::Return {
            expression: expression.map(|e| Box::new(rewrite_captures(*e, captures))),
            pos,
        },
        Statement::Throw(e) => Statement::Throw(rewrite_captures(e, captures)),
        other => other,
    }
}
