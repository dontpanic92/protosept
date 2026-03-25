use crate::ast::{Expression, Identifier};
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::intern::InternedString;
use crate::semantic::Type;

use super::{Generator, SaResult};

impl Generator {
    /// Compute move info for an expression (identifier referring to non-Copy local/param).
    /// Compute move info for an expression. Returns (id, is_param) if the value would be moved.
    pub(crate) fn compute_move_info(&self, expr: &Expression) -> Option<(u32, bool)> {
        if let Expression::Identifier(ident) = expr {
            if let Some(var_id) = self
                .local_scope
                .as_ref()
                .unwrap()
                .find_variable(&ident.name)
            {
                let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                if !ty.is_copy_treated(&self.symbol_table) {
                    return Some((var_id, false));
                }
            } else if let Some(param_id) =
                self.local_scope.as_ref().unwrap().find_param(&ident.name)
            {
                let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                if !ty.is_copy_treated(&self.symbol_table) {
                    return Some((param_id, true));
                }
            }
        }
        None
    }

    /// Process arguments (positional or named) and map them to parameters/fields.
    /// Returns ordered expressions matching the parameter/field order.
    pub(crate) fn process_arguments(
        &self,
        call_name: &str,
        call_line: usize,
        call_col: usize,
        arguments: Vec<(Option<Identifier>, Expression)>,
        param_names: &[InternedString],
        param_defaults: &[Option<Expression>],
    ) -> SaResult<Vec<Expression>> {
        let has_named = arguments.iter().any(|(n, _)| n.is_some());
        let has_positional = arguments.iter().any(|(n, _)| n.is_none());

        if has_named && has_positional {
            return Err(SemanticError::MixedNamedAndPositional {
                name: call_name.to_string(),
                pos: SourcePos::at(call_line, call_col),
            });
        }

        let mut ordered_exprs: Vec<Expression> = Vec::with_capacity(param_names.len());

        if has_named {
            // Named arguments: build a map and order by parameters
            let mut arg_map = std::collections::HashMap::new();
            for (name_opt, expr) in arguments.into_iter() {
                if let Some(name) = name_opt {
                    arg_map.insert(name.name, expr);
                }
            }

            // For each parameter, use provided arg or default
            for (i, param_name) in param_names.iter().enumerate() {
                if let Some(expr) = arg_map.remove(param_name) {
                    ordered_exprs.push(expr);
                } else if let Some(default_expr) = param_defaults.get(i).and_then(|o| o.clone()) {
                    ordered_exprs.push(default_expr);
                } else {
                    return Err(SemanticError::MissingArgument {
                        param_name: param_name.to_string(),
                        func_name: call_name.to_string(),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
            }

            if !arg_map.is_empty() {
                return Err(SemanticError::Other(format!(
                    "Unknown named arguments provided: {:?}",
                    arg_map.keys().collect::<Vec<_>>()
                )));
            }
        } else {
            // Positional arguments: order must match parameters
            if arguments.len() > param_names.len() {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!("expected {} args", param_names.len()),
                    rhs: format!("{} provided", arguments.len()),
                    pos: SourcePos::at(call_line, call_col),
                });
            }

            for (_name_opt, expr) in arguments.into_iter() {
                ordered_exprs.push(expr);
            }

            // Fill missing with defaults
            for i in ordered_exprs.len()..param_names.len() {
                if let Some(default_expr) = param_defaults.get(i).and_then(|o| o.clone()) {
                    ordered_exprs.push(default_expr);
                } else {
                    return Err(SemanticError::MissingArgument {
                        param_name: param_names[i].to_string(),
                        func_name: call_name.to_string(),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
            }
        }

        Ok(ordered_exprs)
    }

    pub(crate) fn push_typed_argument_list(
        &mut self,
        arguments: Vec<Expression>,
        param_types: &[Type],
        call_line: usize,
        call_col: usize,
    ) -> SaResult<()> {
        if arguments.len() != param_types.len() {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("{} args expected", param_types.len()),
                rhs: format!("{} provided", arguments.len()),
                pos: SourcePos::at(call_line, call_col),
            });
        }

        for (expr, param_ty) in arguments.into_iter().zip(param_types.iter()) {
            // Check if this expression involves a move (before consuming it)
            let move_info = self.compute_move_info(&expr);

            let arg_ty = self.generate_expression(expr)?;

            // Mark variable as moved if needed
            if let Some((id, is_param)) = move_info {
                if is_param {
                    self.mark_param_moved(id);
                } else {
                    self.mark_variable_moved(id);
                }
            }

            match (param_ty, &arg_ty) {
                (Type::Reference(param_inner), Type::Reference(arg_inner)) => {
                    if **param_inner != **arg_inner {
                        return Err(SemanticError::TypeMismatch {
                            lhs: arg_ty.to_string(),
                            rhs: param_ty.to_string(),
                            pos: SourcePos::at(call_line, call_col),
                        });
                    }
                }
                (Type::Reference(_), _) => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: arg_ty.to_string(),
                        rhs: param_ty.to_string(),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                (_, Type::Reference(_)) => {
                    // No implicit deref: `ref` values cannot be passed to non-ref parameters.
                    return Err(SemanticError::TypeMismatch {
                        lhs: arg_ty.to_string(),
                        rhs: param_ty.to_string(),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                _ => {
                    // Check type compatibility for non-ref parameters
                    if !self.types_compatible(&arg_ty, param_ty) {
                        return Err(SemanticError::TypeMismatch {
                            lhs: format!("argument type {}", arg_ty.to_string()),
                            rhs: format!("parameter type {}", param_ty.to_string()),
                            pos: SourcePos::at(call_line, call_col),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn push_argument_list(&mut self, arguments: Vec<Expression>) -> SaResult<()> {
        for expr in arguments {
            self.generate_expression(expr)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, Identifier};

    struct DummyProvider;
    impl crate::ModuleProvider for DummyProvider {
        fn load_module(&self, _module_path: &str) -> Option<String> {
            None
        }

        fn clone_boxed(&self) -> Box<dyn crate::ModuleProvider> {
            Box::new(DummyProvider)
        }
    }

    fn mk_gen() -> Generator {
        Generator::new(Box::new(DummyProvider))
    }

    #[test]
    fn mixed_named_positional_args_error() {
        let g = mk_gen();
        let args = vec![
            (None, Expression::IntegerLiteral(1)),
            (
                Some(Identifier {
                    name: "x".into(),
                    line: 0,
                    col: 0,
                }),
                Expression::IntegerLiteral(2),
            ),
        ];
        let res = g.process_arguments("foo", 0, 0, args, &["a".into(), "b".into()], &[None, None]);
        assert!(matches!(
            res,
            Err(SemanticError::MixedNamedAndPositional { .. })
        ));
    }

    #[test]
    fn fills_defaults_for_missing_args() {
        let g = mk_gen();
        let args = vec![(None, Expression::IntegerLiteral(1))];
        let res = g
            .process_arguments(
                "foo",
                0,
                0,
                args,
                &["a".into(), "b".into()],
                &[None, Some(Expression::IntegerLiteral(2))],
            )
            .unwrap();
        assert_eq!(res.len(), 2);
    }

    #[test]
    fn compute_move_info_none_for_literal() {
        let g = mk_gen();
        let expr = Expression::IntegerLiteral(1);
        assert!(g.compute_move_info(&expr).is_none());
    }
}
