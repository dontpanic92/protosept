use crate::ast::{Expression, FunctionCall, Identifier, InterpolatedStringPart};
use crate::errors::SemanticError;
use crate::intern::InternedString;
use crate::semantic::{LocalSymbolScope, PrimitiveType, Type, Variable};

use super::super::{Generator, SaResult};

impl Generator {
    pub(in crate::bytecode::codegen) fn generate_interpolated_string(
        &mut self,
        parts: Vec<InterpolatedStringPart>,
    ) -> SaResult<Type> {
        let concat_id = self.add_string_constant("string.concat");
        let mut has_value = false;

        for part in parts {
            let part_ty = match part {
                InterpolatedStringPart::Literal(value) => self.generate_string_literal(&value)?,
                InterpolatedStringPart::Expr(expr) => {
                    let display_call = Expression::FunctionCall(FunctionCall {
                        callee: Box::new(Expression::FieldAccess {
                            object: Box::new(expr),
                            field: Identifier {
                                name: InternedString::from("display"),
                                line: 0,
                                col: 0,
                            },
                        }),
                        arguments: vec![],
                    });

                    self.generate_expression(display_call)?
                }
            };

            if part_ty != Type::Primitive(PrimitiveType::String) {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&part_ty),
                    rhs: "string".to_string(),
                    pos: None,
                });
            }

            if has_value {
                self.builder.call_host_function(concat_id);
            } else {
                has_value = true;
            }
        }

        if !has_value {
            let empty_id = self.add_string_constant("");
            self.builder.lds(empty_id);
        }

        Ok(Type::Primitive(PrimitiveType::String))
    }

    pub(in crate::bytecode::codegen) fn generate_identifier(&mut self, identifier: Identifier) -> SaResult<Type> {
        // Handle `Self` keyword for type references in methods
        if identifier.name == "Self" {
            if let Some(self_type) = &self.current_self_type {
                return Ok(self_type.clone());
            } else {
                return Err(SemanticError::Other(
                    "Self can only be used inside methods".to_string(),
                ));
            }
        }

        // When inside a function/method, check local scope first
        if let Some(ref mut scope) = self.local_scope {
            // Try to find as local variable
            if let Some(var_id) = scope.find_variable(&identifier.name) {
                if self.is_variable_moved(var_id) {
                    return Err(SemanticError::UseAfterMove {
                        name: identifier.name.to_string(),
                        pos: self.make_pos(identifier.line, identifier.col),
                    });
                }
                self.builder.ldvar(var_id);
                let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                return Ok(ty);
            }

            // Try to find as parameter
            if let Some(param_id) = scope.find_param(&identifier.name) {
                if self.is_param_moved(param_id) {
                    return Err(SemanticError::UseAfterMove {
                        name: identifier.name.to_string(),
                        pos: self.make_pos(identifier.line, identifier.col),
                    });
                }
                self.builder.ldpar(param_id);
                let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                return Ok(ty);
            }
        }

        // Try to find as module-level variable
        if let Some(mod_var) = self.find_module_variable(&identifier.name) {
            let ty = mod_var.ty.clone();
            let var_id = mod_var.var_id;
            self.builder.ldmodvar(var_id);
            return Ok(ty);
        }

        Err(SemanticError::VariableNotFound {
            name: identifier.name.to_string(),
            pos: self.make_pos(identifier.line, identifier.col),
        })
    }

    pub(in crate::bytecode::codegen) fn generate_string_literal(&mut self, value: &str) -> SaResult<Type> {
        let string_index = if let Some(idx) = self.string_constants.iter().position(|s| s == value)
        {
            idx as u32
        } else {
            let idx = self.string_constants.len() as u32;
            self.string_constants.push(value.to_string());
            idx
        };

        self.builder.lds(string_index);
        Ok(Type::Primitive(PrimitiveType::String))
    }

    pub(in crate::bytecode::codegen) fn generate_array_literal(
        &mut self,
        elements: Vec<Expression>,
        pos: (usize, usize),
        expected_element_type: Option<&Type>,
    ) -> SaResult<Type> {
        let (line, col) = pos;

        // Infer element type from first element if non-empty, or use expected type for empty arrays
        let element_type = if elements.is_empty() {
            // Empty array uses expected type from context
            if let Some(expected) = expected_element_type {
                expected.clone()
            } else {
                return Err(SemanticError::Other(format!(
                    "Cannot infer type for empty array literal at {}:{} - expected type annotation required",
                    line, col
                )));
            }
        } else {
            // Generate code for all elements and check they have the same type
            let first_expr_type = self.generate_expression(elements[0].clone())?;

            for element in &elements[1..] {
                let expr_type = self.generate_expression(element.clone())?;
                if expr_type != first_expr_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(&first_expr_type),
                        rhs: self.type_to_string(&expr_type),
                        pos: self.make_pos(line, col),
                    });
                }
            }

            first_expr_type
        };

        // Push element count onto stack
        self.builder.ldi(elements.len() as i64);

        // Call array.new host function
        let string_id = self.add_string_constant("array.new");
        self.builder.call_host_function(string_id);

        Ok(Type::Array(Box::new(element_type)))
    }

    pub(in crate::bytecode::codegen) fn generate_tuple_literal(
        &mut self,
        elements: Vec<Expression>,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let (line, col) = pos;

        if elements.len() < 2 {
            return Err(SemanticError::Other(format!(
                "Tuple must have at least 2 elements at {}:{}",
                line, col
            )));
        }

        let mut element_types = Vec::new();
        for element in &elements {
            let ty = self.generate_expression(element.clone())?;
            element_types.push(ty);
        }

        self.builder.ldi(elements.len() as i64);

        let string_id = self.add_string_constant("tuple.new");
        self.builder.call_host_function(string_id);

        Ok(Type::Tuple(element_types))
    }

    pub(in crate::bytecode::codegen) fn generate_map_literal(
        &mut self,
        pairs: Vec<(Expression, Expression)>,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let (line, col) = pos;

        if pairs.is_empty() {
            return Err(SemanticError::Other(format!(
                "Cannot infer type for empty map literal at {}:{} - use HashMap<K, V>() constructor",
                line, col
            )));
        }

        // Generate code for all key-value pairs and check types are consistent
        let first_key_type = self.generate_expression(pairs[0].0.clone())?;
        let first_val_type = self.generate_expression(pairs[0].1.clone())?;

        for (key_expr, val_expr) in &pairs[1..] {
            let key_type = self.generate_expression(key_expr.clone())?;
            if key_type != first_key_type {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&first_key_type),
                    rhs: self.type_to_string(&key_type),
                    pos: self.make_pos(line, col),
                });
            }
            let val_type = self.generate_expression(val_expr.clone())?;
            if val_type != first_val_type {
                return Err(SemanticError::TypeMismatch {
                    lhs: self.type_to_string(&first_val_type),
                    rhs: self.type_to_string(&val_type),
                    pos: self.make_pos(line, col),
                });
            }
        }

        // Push pair count onto stack
        self.builder.ldi(pairs.len() as i64);

        // Call hashmap.new host function
        let string_id = self.add_string_constant("hashmap.new");
        self.builder.call_host_function(string_id);

        Ok(Type::Map(Box::new(first_key_type), Box::new(first_val_type)))
    }

    pub(in crate::bytecode::codegen) fn generate_array_index(
        &mut self,
        array: Expression,
        index: Expression,
        pos: (usize, usize),
    ) -> SaResult<Type> {
        let (line, col) = pos;

        // Generate code for array/map expression
        let container_type = self.generate_expression(array)?;

        // Check if it's a map type — handle map[key] indexing
        match &container_type {
            Type::Map(key_type, val_type) => {
                let index_type = self.generate_expression(index)?;
                if index_type != **key_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(key_type),
                        rhs: self.type_to_string(&index_type),
                        pos: self.make_pos(line, col),
                    });
                }
                let string_id = self.add_string_constant("hashmap.index");
                self.builder.call_host_function(string_id);
                return Ok(*val_type.clone());
            }
            Type::Reference(inner) => {
                if let Type::Map(key_type, val_type) = inner.as_ref() {
                    let index_type = self.generate_expression(index)?;
                    if index_type != **key_type {
                        return Err(SemanticError::TypeMismatch {
                            lhs: self.type_to_string(key_type),
                            rhs: self.type_to_string(&index_type),
                            pos: self.make_pos(line, col),
                        });
                    }
                    let string_id = self.add_string_constant("hashmap.index");
                    self.builder.call_host_function(string_id);
                    return Ok(*val_type.clone());
                }
            }
            Type::BoxType(inner) => {
                if let Type::Map(key_type, val_type) = inner.as_ref() {
                    self.builder.box_deref();
                    let index_type = self.generate_expression(index)?;
                    if index_type != **key_type {
                        return Err(SemanticError::TypeMismatch {
                            lhs: self.type_to_string(key_type),
                            rhs: self.type_to_string(&index_type),
                            pos: self.make_pos(line, col),
                        });
                    }
                    let string_id = self.add_string_constant("hashmap.index");
                    self.builder.call_host_function(string_id);
                    return Ok(*val_type.clone());
                }
            }
            _ => {}
        }

        // Check that it's an array type
        let element_type = match container_type {
            Type::Array(elem_type) => *elem_type,
            Type::Reference(inner) => match *inner {
                Type::Array(elem_type) => *elem_type,
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "array or HashMap".to_string(),
                        rhs: self.type_to_string(&other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            Type::BoxType(inner) => match *inner {
                Type::Array(elem_type) => {
                    self.builder.box_deref();
                    *elem_type
                }
                other => {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "array or HashMap".to_string(),
                        rhs: self.type_to_string(&other),
                        pos: self.make_pos(line, col),
                    });
                }
            },
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "array or HashMap".to_string(),
                    rhs: self.type_to_string(&container_type),
                    pos: self.make_pos(line, col),
                });
            }
        };

        // Generate code for index expression
        let index_type = self.generate_expression(index)?;

        // Check that index is an integer
        if index_type != Type::Primitive(PrimitiveType::Int) {
            return Err(SemanticError::TypeMismatch {
                lhs: "int".to_string(),
                rhs: self.type_to_string(&index_type),
                pos: self.make_pos(line, col),
            });
        }

        // Call array.index host function
        let string_id = self.add_string_constant("array.index");
        self.builder.call_host_function(string_id);

        Ok(element_type)
    }

    pub(in crate::bytecode::codegen) fn generate_closure(
        &mut self,
        parameters: Vec<crate::ast::Parameter>,
        body: Expression,
        _pos: (usize, usize),
    ) -> SaResult<Type> {
        // Resolve parameter types
        let params: Vec<Variable> = parameters
            .iter()
            .map(|p| {
                self.get_semantic_type(&p.arg_type).map(|ty| Variable {
                    name: p.name.name.clone(),
                    ty,
                    is_mutable: false,
                })
            })
            .collect::<SaResult<Vec<_>>>()?;

        let param_types: Vec<Type> = params.iter().map(|v| v.ty.clone()).collect();
        let param_names: std::collections::HashSet<InternedString> =
            params.iter().map(|v| v.name.clone()).collect();

        // Collect free variables: names referenced in body that exist in the
        // enclosing scope but are not closure parameters
        let mut free_vars: Vec<(InternedString, Type, bool)> = Vec::new(); // (name, type, is_param)
        let mut seen = std::collections::HashSet::new();
        let referenced = Self::collect_identifiers(&body);

        if let Some(scope) = &self.local_scope {
            for name in &referenced {
                if param_names.contains(name) || seen.contains(name) {
                    continue;
                }
                if let Some(var_id) = scope.find_variable(name) {
                    let ty = scope.get_variable_type(var_id).clone();
                    free_vars.push((name.clone(), ty, false));
                    seen.insert(name.clone());
                } else if let Some(param_id) = scope.find_param(name) {
                    let ty = scope.get_param_type(param_id).clone();
                    free_vars.push((name.clone(), ty, true));
                    seen.insert(name.clone());
                }
            }
        }

        let capture_count = free_vars.len() as u32;

        // Build the closure's parameter list: captures first, then declared params
        let mut closure_params: Vec<Variable> = free_vars
            .iter()
            .map(|(name, ty, _)| Variable {
                name: name.clone(),
                ty: ty.clone(),
                is_mutable: false,
            })
            .collect();
        closure_params.extend(params);

        // Emit jump to skip over the closure body
        let jump_placeholder = self.builder.next_address();
        self.builder.jmp(0);

        let func_addr = self.builder.next_address();

        // Generate closure body with captures + params in scope
        let saved_scope = self.local_scope.take();
        self.local_scope = Some(LocalSymbolScope::new(closure_params));

        let body_type = self.generate_expression(body)?;
        self.builder.ret();

        self.local_scope = saved_scope;

        // Patch the jump
        let after_body = self.builder.next_address();
        self.builder.patch_jump_address(jump_placeholder, after_body);

        // Push captured values onto the stack (in order)
        for (name, _ty, is_param) in &free_vars {
            if let Some(scope) = &self.local_scope {
                if *is_param {
                    if let Some(param_id) = scope.find_param(name) {
                        self.builder.ldpar(param_id);
                    }
                } else {
                    if let Some(var_id) = scope.find_variable(name) {
                        self.builder.ldvar(var_id);
                    }
                }
            }
        }

        self.builder.make_closure(func_addr, capture_count);

        Ok(Type::Function {
            params: param_types,
            return_type: Box::new(body_type),
        })
    }

    /// Collect all identifier names referenced in an expression (shallow scan)
    fn collect_identifiers(expr: &Expression) -> Vec<InternedString> {
        let mut names = Vec::new();
        Self::collect_identifiers_recursive(expr, &mut names);
        names
    }

    fn collect_identifiers_recursive(expr: &Expression, names: &mut Vec<InternedString>) {
        match expr {
            Expression::Identifier(id) => {
                names.push(id.name.clone());
            }
            Expression::Binary { left, right, .. } => {
                Self::collect_identifiers_recursive(left, names);
                Self::collect_identifiers_recursive(right, names);
            }
            Expression::Unary { right, .. } => {
                Self::collect_identifiers_recursive(right, names);
            }
            Expression::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::collect_identifiers_recursive(condition, names);
                Self::collect_identifiers_recursive(then_branch, names);
                if let Some(eb) = else_branch {
                    Self::collect_identifiers_recursive(eb, names);
                }
            }
            Expression::FunctionCall(call) => {
                Self::collect_identifiers_recursive(&call.callee, names);
                for (_, arg) in &call.arguments {
                    Self::collect_identifiers_recursive(arg, names);
                }
            }
            Expression::FieldAccess { object, .. } => {
                Self::collect_identifiers_recursive(object, names);
            }
            Expression::Block(stmts) => {
                for stmt in stmts {
                    match stmt {
                        crate::ast::Statement::Expression(e) => {
                            Self::collect_identifiers_recursive(e, names);
                        }
                        crate::ast::Statement::Let { expression, .. } => {
                            Self::collect_identifiers_recursive(expression, names);
                        }
                        crate::ast::Statement::Return(e) => {
                            Self::collect_identifiers_recursive(e, names);
                        }
                        crate::ast::Statement::Throw(e) => {
                            Self::collect_identifiers_recursive(e, names);
                        }
                        _ => {}
                    }
                }
            }
            Expression::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    Self::collect_identifiers_recursive(elem, names);
                }
            }
            Expression::ArrayIndex { array, index, .. } => {
                Self::collect_identifiers_recursive(array, names);
                Self::collect_identifiers_recursive(index, names);
            }
            Expression::ForceUnwrap { operand, .. } => {
                Self::collect_identifiers_recursive(operand, names);
            }
            Expression::Ref(inner) => {
                Self::collect_identifiers_recursive(inner, names);
            }
            Expression::Cast { expression, .. } => {
                Self::collect_identifiers_recursive(expression, names);
            }
            Expression::Loop { body, .. } | Expression::While { body, .. } => {
                Self::collect_identifiers_recursive(body, names);
            }
            Expression::Closure { body, .. } => {
                Self::collect_identifiers_recursive(body, names);
            }
            Expression::Try { try_block, else_arms } => {
                Self::collect_identifiers_recursive(try_block, names);
                for arm in else_arms {
                    Self::collect_identifiers_recursive(&arm.expression, names);
                }
            }
            Expression::Match { scrutinee, arms } => {
                Self::collect_identifiers_recursive(scrutinee, names);
                for arm in arms {
                    Self::collect_identifiers_recursive(&arm.expression, names);
                }
            }
            _ => {}
        }
    }
}
