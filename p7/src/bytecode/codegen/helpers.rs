use crate::ast::Type as ParsedType;
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::intern::InternedString;
use crate::{
    ast::{Expression, Identifier, Pattern},
    semantic::{PrimitiveType, SymbolId, Type, TypeDefinition},
};

use super::{Generator, SaResult};

/// Parsed contents of a `@foreign(...)` attribute on a proto declaration.
///
/// Required keys are `dispatcher` and `type_tag`; `finalizer` and `uuid`
/// are optional. All keys are `Option` so the parser can be permissive —
/// full validation (required-key presence, `type_tag` uniqueness, allowed
/// value types) is performed by `Generator::validate_foreign_attrs`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ForeignAttrs {
    pub dispatcher: Option<InternedString>,
    pub finalizer: Option<InternedString>,
    pub type_tag: Option<InternedString>,
    /// Optional COM-style UUID (16 bytes) string identifying the
    /// underlying interface. Stored verbatim; the runtime parses it.
    pub uuid: Option<InternedString>,
}

impl Generator {
    /// Look up a symbol in scope, returning an error if not found
    pub(super) fn require_symbol_in_scope(
        &self,
        name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<SymbolId> {
        self.symbol_table.find_symbol_in_scope(name).ok_or_else(|| {
            SemanticError::FunctionNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            }
        })
    }

    /// Look up a type in scope, returning an error if not found
    pub(super) fn require_type_in_scope(
        &self,
        name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        self.symbol_table
            .find_type_in_scope(name)
            .ok_or_else(|| SemanticError::TypeNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            })
    }

    /// Look up a type in scope from an Identifier, returning an error if not found
    pub(super) fn require_type_from_identifier(&self, ident: &Identifier) -> SaResult<Type> {
        self.require_type_in_scope(&ident.name, ident.line, ident.col)
    }

    /// Resolve a list of parsed type arguments to semantic types
    pub(super) fn resolve_type_args(&mut self, type_args: &[ParsedType]) -> SaResult<Vec<Type>> {
        type_args
            .iter()
            .map(|arg| self.get_semantic_type(arg))
            .collect()
    }

    /// Validate that the number of type arguments matches the expected count
    pub(super) fn validate_type_arg_count(
        expected: usize,
        actual: usize,
        line: usize,
        col: usize,
    ) -> SaResult<()> {
        if expected != actual {
            return Err(SemanticError::TypeMismatch {
                lhs: format!("{} type parameters", expected),
                rhs: format!("{} type arguments", actual),
                pos: SourcePos::at(line, col),
            });
        }
        Ok(())
    }

    /// Helper to add a string constant to the pool and return its index
    pub(super) fn add_string_constant(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.string_constant_ids.get(s) {
            return *idx;
        }

        let idx = self.string_constants.len() as u32;
        self.string_constants.push(s.to_string());
        self.string_constant_ids.insert(s.to_string(), idx);
        idx
    }

    pub(super) fn handle_primitive_method_call(
        &mut self,
        prim_ty: &PrimitiveType,
        field: &Identifier,
        arguments: &Vec<(Option<Identifier>, Expression)>,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        self.load_builtin();
        match prim_ty {
            PrimitiveType::String => {
                let builtin = &self.imported_modules["builtin"];
                let method = {
                    let string = builtin.symbols.iter().find(|s| s.name == "string").unwrap();
                    string.children.iter().find(|s| s.0 == &field.name)
                };

                if method.is_none() {
                    return Err(SemanticError::FunctionNotFound {
                        name: format!("string.{}", field.name),
                        pos: field.pos(),
                    });
                }

                let method_id = *method.unwrap().1;
                let method_symbol = builtin.symbols.get(method_id as usize).unwrap();
                let func_id = method_symbol.get_func_id().unwrap();
                let function_def = builtin.functions.get(func_id as usize).unwrap().clone();

                let intrinsic_name =
                    Self::extract_intrinsic_name(&function_def.attributes).unwrap();

                let param_names = function_def.param_names.clone();
                let param_defaults: Vec<Option<Expression>> = function_def.param_defaults.clone();

                // Use shared argument processing logic
                let ordered_exprs = self.process_arguments(
                    &format!("string.{}", field.name),
                    call_line,
                    call_col,
                    arguments.clone(),
                    &param_names[1..],
                    &param_defaults[1..],
                )?;

                // receivers already on stack
                self.push_typed_argument_list(
                    ordered_exprs,
                    &function_def.params[1..],
                    call_line,
                    call_col,
                )?;
                let string_id = self.add_string_constant(&intrinsic_name);
                self.builder.call_host_function(string_id);
                Ok(function_def.return_type.clone())
            }
            PrimitiveType::Int
            | PrimitiveType::Float
            | PrimitiveType::Bool
            | PrimitiveType::Char
            | PrimitiveType::Unit => {
                if field.name != "display" {
                    return Err(SemanticError::FunctionNotFound {
                        name: format!("{:?}.{}", prim_ty, field.name),
                        pos: field.pos(),
                    });
                }

                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args expected".to_string(),
                        rhs: format!("{} provided", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }

                let intrinsic_name = match prim_ty {
                    PrimitiveType::Int => "display.int",
                    PrimitiveType::Float => "display.float",
                    PrimitiveType::Bool => "display.bool",
                    PrimitiveType::Char => "display.char",
                    PrimitiveType::Unit => "display.unit",
                    PrimitiveType::String => unreachable!(),
                };

                let string_id = self.add_string_constant(intrinsic_name);
                self.builder.call_host_function(string_id);
                Ok(Type::Primitive(PrimitiveType::String))
            }
        }
    }

    pub(super) fn handle_array_method_call(
        &mut self,
        object_ty: &Type,
        field: &Identifier,
        arguments: &Vec<(Option<Identifier>, Expression)>,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        self.load_builtin();

        // Extract the element type from the array receiver
        let element_type = match object_ty {
            Type::Array(inner) => inner.as_ref().clone(),
            Type::Reference(inner) => match inner.as_ref() {
                Type::Array(elem) => elem.as_ref().clone(),
                _ => Type::Primitive(PrimitiveType::Unit),
            },
            Type::BoxType(inner) => match inner.as_ref() {
                Type::Array(elem) => elem.as_ref().clone(),
                _ => Type::Primitive(PrimitiveType::Unit),
            },
            _ => Type::Primitive(PrimitiveType::Unit),
        };

        // Extract all needed data from the builtin module first to avoid borrow issues
        let (
            intrinsic_name,
            param_names,
            param_defaults,
            generic_param_types,
            generic_return_type,
            return_type,
            is_self_return,
        ) = {
            let builtin = &self.imported_modules["builtin"];
            let method = {
                let array_struct = builtin.symbols.iter().find(|s| s.name == "array").unwrap();
                array_struct.children.iter().find(|s| s.0 == &field.name)
            };

            if method.is_none() {
                return Err(SemanticError::FunctionNotFound {
                    name: format!("array.{}", field.name),
                    pos: field.pos(),
                });
            }

            let method_id = *method.unwrap().1;
            let method_symbol = builtin.symbols.get(method_id as usize).unwrap();
            let func_id = method_symbol.get_func_id().unwrap();
            let function_def = builtin.functions.get(func_id as usize).unwrap().clone();

            let intrinsic_name = Self::extract_intrinsic_name(&function_def.attributes).unwrap();

            // Check if return type is the builtin array struct (meaning "Self")
            let is_self_return = match &function_def.return_type {
                Type::Struct(id) => {
                    builtin.types.iter().position(|t| {
                        matches!(t, TypeDefinition::Struct(s) if s.qualified_name == "builtin.array")
                    }) == Some(*id as usize)
                }
                _ => false,
            };

            (
                intrinsic_name,
                function_def.param_names.clone(),
                function_def.param_defaults.clone(),
                function_def.generic_param_types.clone(),
                function_def.generic_return_type.clone(),
                function_def.return_type.clone(),
                is_self_return,
            )
        };

        let substitution = if generic_param_types.is_some() {
            // Build substitution map: T -> element_type, Self -> array<element_type>
            let mut substitution: std::collections::HashMap<InternedString, ParsedType> =
                std::collections::HashMap::new();
            let parsed_element_type = self.type_to_parsed_type(&element_type);
            substitution.insert(InternedString::from("T"), parsed_element_type.clone());

            // Self should resolve to array<T> with the actual element type
            let self_type = ParsedType::Generic {
                base: crate::ast::Identifier {
                    name: InternedString::from("array"),
                    line: 0,
                    col: 0,
                },
                type_args: vec![parsed_element_type],
            };
            substitution.insert(InternedString::from("Self"), self_type);
            substitution
        } else {
            std::collections::HashMap::new()
        };

        // Resolve parameter types by substituting T with the actual element type
        let params = if let Some(ref generic_params) = generic_param_types {
            // Substitute and resolve each parameter type
            let mut resolved_params = Vec::new();
            for parsed_param in generic_params {
                let substituted = self.substitute_parsed_type(parsed_param, &substitution);
                let resolved = self.get_semantic_type(&substituted)?;
                resolved_params.push(resolved);
            }
            resolved_params
        } else {
            // No generic params - use empty (shouldn't happen for array methods)
            Vec::new()
        };

        let resolved_return_type = if let Some(parsed_return_type) = generic_return_type {
            let substituted = self.substitute_parsed_type(&parsed_return_type, &substitution);
            Some(self.get_semantic_type(&substituted)?)
        } else {
            None
        };

        // If the receiver is a box but the method expects a non-box self, deref first.
        if let (Type::BoxType(_), Some(expected_self)) = (object_ty, params.first())
            && !matches!(expected_self, Type::BoxType(_))
        {
            self.builder.box_deref();
        }

        // Use shared argument processing logic
        let ordered_exprs = self.process_arguments(
            &format!("array.{}", field.name),
            call_line,
            call_col,
            arguments.clone(),
            &param_names[1..], // Skip self parameter
            &param_defaults[1..],
        )?;

        // Receiver already on stack from generate_expression
        // Push additional arguments
        self.push_typed_argument_list(
            ordered_exprs,
            &params[1..], // Skip self parameter
            call_line,
            call_col,
        )?;

        // Call the intrinsic host function
        let string_id = self.add_string_constant(&intrinsic_name);
        self.builder.call_host_function(string_id);

        // Resolve the return type: if is_self_return is true, it means the method
        // returns "Self" which should be the actual array type.
        let final_return_type = if let Some(resolved) = resolved_return_type {
            resolved
        } else if is_self_return {
            // Extract the actual array type from object_ty
            match object_ty {
                Type::Array(_) => object_ty.clone(),
                Type::Reference(inner) => match inner.as_ref() {
                    Type::Array(_) => inner.as_ref().clone(),
                    _ => return_type,
                },
                Type::BoxType(inner) => match inner.as_ref() {
                    Type::Array(_) => inner.as_ref().clone(),
                    _ => return_type,
                },
                _ => return_type,
            }
        } else {
            return_type
        };

        Ok(final_return_type)
    }

    /// Handle method calls on HashMap values (Map type)
    pub(super) fn handle_hashmap_method_call(
        &mut self,
        key_type: &Type,
        val_type: &Type,
        object_ty: &Type,
        field: &Identifier,
        arguments: &Vec<(Option<Identifier>, Expression)>,
        call_line: usize,
        call_col: usize,
    ) -> SaResult<Type> {
        // If the receiver is a box, deref for non-mutating methods
        let is_box = matches!(object_ty, Type::BoxType(_));
        let needs_deref = is_box && !matches!(field.name.as_str(), "set" | "remove");
        if needs_deref {
            self.builder.box_deref();
        }

        let (intrinsic_name, return_type): (&str, Type) = match field.name.as_str() {
            "len" => {
                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                ("hashmap.len", Type::Primitive(PrimitiveType::Int))
            }
            "get" => {
                if arguments.len() != 1 {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "1 arg".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                let arg_type = self.generate_expression(arguments[0].1.clone())?;
                if arg_type != *key_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(key_type),
                        rhs: self.type_to_string(&arg_type),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                ("hashmap.get", Type::Nullable(Box::new(val_type.clone())))
            }
            "set" => {
                if !is_box {
                    return Err(SemanticError::Other(format!(
                        "hashmap.set requires box<HashMap> receiver at line {} column {}",
                        call_line, call_col
                    )));
                }
                if arguments.len() != 2 {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "2 args".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                // BoxRef is already on stack; push key and value, then call set
                // The host function mutates the box heap in-place (like array.push)
                let key_arg_type = self.generate_expression(arguments[0].1.clone())?;
                if key_arg_type != *key_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(key_type),
                        rhs: self.type_to_string(&key_arg_type),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                let val_arg_type = self.generate_expression(arguments[1].1.clone())?;
                if val_arg_type != *val_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(val_type),
                        rhs: self.type_to_string(&val_arg_type),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                let string_id = self.add_string_constant("hashmap.set");
                self.builder.call_host_function(string_id);
                return Ok(Type::Primitive(PrimitiveType::Unit));
            }
            "remove" => {
                if !is_box {
                    return Err(SemanticError::Other(format!(
                        "hashmap.remove requires box<HashMap> receiver at line {} column {}",
                        call_line, call_col
                    )));
                }
                if arguments.len() != 1 {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "1 arg".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                // BoxRef is already on stack; push key, call remove
                let arg_type = self.generate_expression(arguments[0].1.clone())?;
                if arg_type != *key_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(key_type),
                        rhs: self.type_to_string(&arg_type),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                let string_id = self.add_string_constant("hashmap.remove");
                self.builder.call_host_function(string_id);
                return Ok(Type::Nullable(Box::new(val_type.clone())));
            }
            "contains_key" => {
                if arguments.len() != 1 {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "1 arg".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                let arg_type = self.generate_expression(arguments[0].1.clone())?;
                if arg_type != *key_type {
                    return Err(SemanticError::TypeMismatch {
                        lhs: self.type_to_string(key_type),
                        rhs: self.type_to_string(&arg_type),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                ("hashmap.contains_key", Type::Primitive(PrimitiveType::Bool))
            }
            "keys" => {
                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                ("hashmap.keys", Type::Array(Box::new(key_type.clone())))
            }
            "values" => {
                if !arguments.is_empty() {
                    return Err(SemanticError::TypeMismatch {
                        lhs: "0 args".to_string(),
                        rhs: format!("{} args", arguments.len()),
                        pos: SourcePos::at(call_line, call_col),
                    });
                }
                ("hashmap.values", Type::Array(Box::new(val_type.clone())))
            }
            _ => {
                return Err(SemanticError::FunctionNotFound {
                    name: format!("HashMap.{}", field.name),
                    pos: field.pos(),
                });
            }
        };

        let string_id = self.add_string_constant(intrinsic_name);
        self.builder.call_host_function(string_id);
        Ok(return_type)
    }

    /// Helper to mark a local variable as moved
    pub(super) fn mark_variable_moved(&mut self, var_id: u32) {
        self.moved_variables.insert(var_id);
    }

    /// Helper to mark a parameter as moved
    pub(super) fn mark_param_moved(&mut self, param_id: u32) {
        self.moved_params.insert(param_id);
    }

    /// Helper to check if a local variable has been moved
    pub(super) fn is_variable_moved(&self, var_id: u32) -> bool {
        self.moved_variables.contains(&var_id)
    }

    /// Helper to check if a parameter has been moved
    pub(super) fn is_param_moved(&self, param_id: u32) -> bool {
        self.moved_params.contains(&param_id)
    }

    /// Helper to clear moved tracking when entering a new function scope
    pub(super) fn clear_moved_variables(&mut self) {
        self.moved_variables.clear();
        self.moved_params.clear();
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
                    name: name.name.to_string(),
                    pos: name.pos(),
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
            Pattern::EnumVariant { .. }
            | Pattern::StructPattern { .. }
            | Pattern::TuplePattern { .. } => Err(SemanticError::Other(
                "Destructuring patterns cannot be converted to expressions".to_string(),
            )),
        }
    }

    pub(super) fn validate_no_intrinsic_or_foreign_attr(
        attributes: &[crate::ast::Attribute],
        target: &str,
    ) -> SaResult<()> {
        for attr in attributes {
            if attr.name.name == "intrinsic" || attr.name.name == "foreign" {
                return Err(SemanticError::Other(format!(
                    "@{} is not valid on {} declarations at line {} column {}",
                    attr.name.name, target, attr.name.line, attr.name.col
                )));
            }
        }
        Ok(())
    }

    pub(super) fn validate_proto_attrs(attributes: &[crate::ast::Attribute]) -> SaResult<()> {
        for attr in attributes {
            if attr.name.name == "intrinsic" {
                return Err(SemanticError::Other(format!(
                    "@intrinsic is not valid on proto declarations at line {} column {}",
                    attr.name.line, attr.name.col
                )));
            }
        }
        Ok(())
    }

    pub(super) fn validate_intrinsic_name(
        attributes: &[crate::ast::Attribute],
    ) -> SaResult<Option<InternedString>> {
        let mut found = None;
        for attr in attributes {
            if attr.name.name != "intrinsic" {
                continue;
            }
            if found.is_some() {
                return Err(SemanticError::Other(format!(
                    "Duplicate @intrinsic attribute at line {} column {}",
                    attr.name.line, attr.name.col
                )));
            }
            if attr.arguments.len() != 1 {
                return Err(SemanticError::Other(format!(
                    "@intrinsic requires exactly one string argument at line {} column {}",
                    attr.name.line, attr.name.col
                )));
            }

            let (name_opt, expr) = &attr.arguments[0];
            if let Some(name) = name_opt
                && name.name != "name"
            {
                return Err(SemanticError::Other(format!(
                    "@intrinsic unknown argument '{}' at line {} column {}",
                    name.name, name.line, name.col
                )));
            }

            let Expression::StringLiteral(s) = expr else {
                return Err(SemanticError::Other(format!(
                    "@intrinsic argument must be a string literal at line {} column {}",
                    attr.name.line, attr.name.col
                )));
            };
            found = Some(s.clone());
        }
        Ok(found)
    }

    pub(super) fn extract_intrinsic_name(
        attributes: &[crate::ast::Attribute],
    ) -> Option<InternedString> {
        Self::validate_intrinsic_name(attributes).ok().flatten()
    }

    /// Parsed contents of a `@foreign(...)` attribute. Required keys are
    /// `dispatcher` and `type_tag`; `finalizer` is optional.
    pub(super) fn extract_foreign_attrs(
        attributes: &[crate::ast::Attribute],
    ) -> SaResult<Option<ForeignAttrs>> {
        let mut found = false;
        let mut out = ForeignAttrs::default();
        for attr in attributes {
            if attr.name.name != "foreign" {
                continue;
            }
            if found {
                return Err(SemanticError::Other(format!(
                    "Duplicate @foreign attribute at line {} column {}",
                    attr.name.line, attr.name.col
                )));
            }
            found = true;
            for (name_opt, expr) in &attr.arguments {
                let key = match name_opt {
                    Some(id) => id.name.clone(),
                    None => {
                        return Err(SemanticError::Other(format!(
                            "@foreign arguments must be named at line {} column {}",
                            attr.name.line, attr.name.col
                        )));
                    }
                };
                let value = match expr {
                    Expression::StringLiteral(s) => s.clone(),
                    _ => {
                        return Err(SemanticError::Other(format!(
                            "@foreign argument '{}' must be a string literal at line {} column {}",
                            key, attr.name.line, attr.name.col
                        )));
                    }
                };
                if value.is_empty() {
                    return Err(SemanticError::Other(format!(
                        "@foreign argument '{}' cannot be empty at line {} column {}",
                        key, attr.name.line, attr.name.col
                    )));
                }

                let target = match key.as_str() {
                    "dispatcher" => &mut out.dispatcher,
                    "finalizer" => &mut out.finalizer,
                    "type_tag" => &mut out.type_tag,
                    "uuid" => &mut out.uuid,
                    _ => {
                        return Err(SemanticError::Other(format!(
                            "@foreign unknown argument '{}' at line {} column {}",
                            key, attr.name.line, attr.name.col
                        )));
                    }
                };
                if target.is_some() {
                    return Err(SemanticError::Other(format!(
                        "Duplicate @foreign argument '{}' at line {} column {}",
                        key, attr.name.line, attr.name.col
                    )));
                }
                if key == "uuid" && !Self::is_valid_uuid_literal(value.as_str()) {
                    return Err(SemanticError::Other(format!(
                        "@foreign uuid '{}' is not a valid UUID at line {} column {}",
                        value, attr.name.line, attr.name.col
                    )));
                }
                *target = Some(value);
            }
        }
        if found { Ok(Some(out)) } else { Ok(None) }
    }

    fn is_valid_uuid_literal(value: &str) -> bool {
        let bytes = value.as_bytes();
        if bytes.len() != 36 {
            return false;
        }
        for (idx, byte) in bytes.iter().enumerate() {
            if matches!(idx, 8 | 13 | 18 | 23) {
                if *byte != b'-' {
                    return false;
                }
            } else if !byte.is_ascii_hexdigit() {
                return false;
            }
        }
        true
    }

    /// Resolve a protocol identifier to its TypeId.
    ///
    /// Dotted names like `radiance.IDirector` are routed through
    /// `resolve_qualified_type_name`, which walks the import chain,
    /// imports the type into the current module if needed, and inserts
    /// a qualified-name symbol. Without this, struct conformance lists
    /// (`struct[radiance.IDirector] X { ... }`) fail with
    /// `TypeNotFound` unless an unrelated typed site forces the
    /// import first.
    pub(super) fn resolve_proto_identifier(
        &mut self,
        proto_name: &Identifier,
    ) -> SaResult<crate::semantic::TypeId> {
        let proto_type = if proto_name.name.contains('.') {
            self.resolve_qualified_type_name(
                &proto_name.name,
                proto_name.line,
                proto_name.col,
            )?
        } else {
            self.symbol_table
                .find_type_in_scope(&proto_name.name)
                .ok_or_else(|| SemanticError::TypeNotFound {
                    name: proto_name.name.to_string(),
                    pos: proto_name.pos(),
                })?
        };

        match proto_type {
            Type::Proto(proto_id) => Ok(proto_id),
            _ => Err(SemanticError::Other(format!(
                "Expected protocol name, found type '{}' at line {} column {}",
                proto_name.name, proto_name.line, proto_name.col
            ))),
        }
    }
}
