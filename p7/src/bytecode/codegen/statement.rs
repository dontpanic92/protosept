use crate::ast::{
    Attribute, EnumVariant, FunctionDeclaration, Identifier, Pattern, ProtoMethod, StructField,
    StructMethod, TypeParameter,
};
use crate::bytecode::Module;
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::intern::InternedString;
use crate::{
    ast::{Expression, Statement},
    semantic::{
        Enum, PrimitiveType, Proto, Struct, Symbol, SymbolKind, Type, TypeDefinition, TypeId,
    },
};

use super::{Generator, SaResult};

impl Generator {
    pub(super) fn generate_statement(&mut self, statement: Statement) -> SaResult<Type> {
        match statement {
            Statement::Let {
                is_pub,
                is_mutable,
                identifier,
                type_annotation,
                expression,
            } => self.generate_let(is_pub, is_mutable, identifier, type_annotation, expression),
            Statement::LetDestructure {
                is_mutable,
                pattern,
                expression,
            } => self.generate_let_destructure(is_mutable, pattern, expression),
            Statement::Expression(expression) => self.generate_expression(expression),
            Statement::FunctionDeclaration(declaration) => self.generate_function_decl(declaration),
            Statement::Throw(expression) => self.generate_throw(expression),
            Statement::EnumDeclaration {
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                values,
                methods,
            } => self.generate_enum_decl(
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                values,
                methods,
            ),
            Statement::StructDeclaration {
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                fields,
                methods,
            } => self.generate_struct_decl(
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                fields,
                methods,
            ),
            Statement::ProtoDeclaration {
                is_pub,
                name,
                attributes,
                methods,
            } => self.generate_proto_decl(is_pub, name, attributes, methods),
            Statement::Return { expression, pos } => {
                self.generate_return(expression.map(|expr| *expr), pos)
            }
            Statement::Import { module_path, alias } => self.generate_import(module_path, alias),
        }
    }

    fn generate_let(
        &mut self,
        is_pub: bool,
        is_mutable: bool,
        identifier: Identifier,
        type_annotation: Option<crate::ast::Type>,
        expression: Expression,
    ) -> SaResult<Type> {
        // pub let is only valid at module level (handled by generate() before calling generate_statement)
        if is_pub {
            return Err(SemanticError::Other(format!(
                "'pub' on let binding '{}' is only allowed at module level",
                identifier.name
            )));
        }

        // Check if this expression involves a move (before consuming it)
        let move_info = self.compute_move_info(&expression);

        // Pre-compute expected type from annotation for type inference
        let expected_type = if let Some(ref annotation) = type_annotation {
            Some(self.get_semantic_type(annotation)?)
        } else {
            None
        };

        let inferred_ty =
            self.generate_expression_with_expected_type(expression, expected_type.as_ref())?;

        // Mark variable as moved if needed
        if let Some((id, is_param)) = move_info {
            if is_param {
                self.mark_param_moved(id);
            } else {
                self.mark_variable_moved(id);
            }
        }

        // Validate type annotation if provided
        let final_ty = if let Some(annotated_ty) = expected_type {
            // Check if inferred type is compatible with annotation
            if !self.types_compatible(&inferred_ty, &annotated_ty) {
                return Err(SemanticError::TypeMismatch {
                    lhs: format!("inferred type {}", inferred_ty.to_string()),
                    rhs: format!("annotated type {}", annotated_ty.to_string()),
                    pos: identifier.pos(),
                });
            }

            // Use the annotated type (which may be more specific, e.g., float when int was inferred)
            annotated_ty
        } else {
            inferred_ty
        };

        let var_id = self
            .local_scope
            .as_mut()
            .unwrap()
            .add_variable(identifier.name.clone(), final_ty, is_mutable)
            .map_err(|_| SemanticError::VariableOutsideFunction {
                name: identifier.name.to_string(),
                pos: identifier.pos(),
            })?;

        self.builder.stvar(var_id);
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_let_destructure(
        &mut self,
        is_mutable: bool,
        pattern: Pattern,
        expression: Expression,
    ) -> SaResult<Type> {
        match &pattern {
            Pattern::StructPattern {
                struct_name,
                field_patterns,
            } => {
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

                // Generate RHS expression (pushes struct value on stack)
                self.generate_expression(expression)?;

                // Extract and bind each field
                for (field_idx, sub_pat) in field_patterns.iter().enumerate() {
                    if !sub_pat.is_wildcard()
                        && let Pattern::Identifier(id) = sub_pat
                    {
                        // Dup for all but nothing extra needed
                        self.builder.dup();
                        self.builder.ldfield(field_idx as u32);
                        let field_ty = struct_def.fields[field_idx].1.clone();
                        let var_id = self
                            .local_scope
                            .as_mut()
                            .unwrap()
                            .add_variable(id.name.clone(), field_ty, is_mutable)
                            .map_err(|_| SemanticError::VariableOutsideFunction {
                                name: id.name.to_string(),
                                pos: id.pos(),
                            })?;
                        self.builder.stvar(var_id);
                    }
                }

                // Pop the struct value from the stack
                self.builder.pop();

                Ok(Type::Primitive(PrimitiveType::Unit))
            }

            Pattern::EnumVariant {
                enum_name,
                variant_name,
                sub_patterns,
            } => {
                // Try direct lookup, then qualified name for cross-module types
                let resolved_type = self.require_type_from_identifier(enum_name).or_else(|_| {
                    let qualified = format!("{}.{}", enum_name.name, variant_name.name);
                    self.resolve_qualified_type_name(&qualified, enum_name.line, enum_name.col)
                })?;

                match resolved_type {
                    Type::Struct(struct_type_id) => {
                        // Cross-module struct destructuring: let types.Pos(r, c) = expr
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

                        self.generate_expression(expression)?;

                        for (field_idx, sub_pat) in sub_patterns.iter().enumerate() {
                            if !sub_pat.is_wildcard()
                                && let Pattern::Identifier(id) = sub_pat
                            {
                                self.builder.dup();
                                self.builder.ldfield(field_idx as u32);
                                let field_ty = struct_def.fields[field_idx].1.clone();
                                let var_id = self
                                    .local_scope
                                    .as_mut()
                                    .unwrap()
                                    .add_variable(id.name.clone(), field_ty, is_mutable)
                                    .map_err(|_| SemanticError::VariableOutsideFunction {
                                        name: id.name.to_string(),
                                        pos: id.pos(),
                                    })?;
                                self.builder.stvar(var_id);
                            }
                        }

                        self.builder.pop();
                        Ok(Type::Primitive(PrimitiveType::Unit))
                    }
                    Type::Enum(enum_type_id) => {
                        let enum_def = match self.symbol_table.get_type(enum_type_id) {
                            TypeDefinition::Enum(e) => e.clone(),
                            _ => {
                                return Err(SemanticError::Other(
                                    "Expected enum type definition".to_string(),
                                ));
                            }
                        };

                        let variant_opt = enum_def
                            .variants
                            .iter()
                            .enumerate()
                            .find(|(_, (name, _))| name == &variant_name.name);

                        let (_variant_index, field_types) =
                            if let Some((idx, (_, types))) = variant_opt {
                                (idx, types.clone())
                            } else {
                                return Err(SemanticError::TypeMismatch {
                                    lhs: format!("Enum '{}'", enum_def.qualified_name),
                                    rhs: format!("Unknown variant '{}'", variant_name.name),
                                    pos: variant_name.pos(),
                                });
                            };

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

                        // Generate RHS expression
                        self.generate_expression(expression)?;

                        // Extract and bind each payload field
                        for (field_idx, sub_pat) in sub_patterns.iter().enumerate() {
                            if !sub_pat.is_wildcard()
                                && let Pattern::Identifier(id) = sub_pat
                            {
                                self.builder.dup();
                                self.builder.ldfield((field_idx + 1) as u32);
                                let field_ty = field_types[field_idx].clone();
                                let var_id = self
                                    .local_scope
                                    .as_mut()
                                    .unwrap()
                                    .add_variable(id.name.clone(), field_ty, is_mutable)
                                    .map_err(|_| SemanticError::VariableOutsideFunction {
                                        name: id.name.to_string(),
                                        pos: id.pos(),
                                    })?;
                                self.builder.stvar(var_id);
                            }
                        }

                        // Pop the enum value from the stack
                        self.builder.pop();

                        Ok(Type::Primitive(PrimitiveType::Unit))
                    }
                    _ => Err(SemanticError::TypeMismatch {
                        lhs: "Enum or Struct type".to_string(),
                        rhs: format!(
                            "'{}.{}' is neither an enum nor a struct",
                            enum_name.name, variant_name.name
                        ),
                        pos: enum_name.pos(),
                    }),
                }
            }

            Pattern::TuplePattern { sub_patterns } => {
                // Generate RHS expression (should be a tuple)
                let rhs_type = self.generate_expression(expression)?;

                let element_types = match &rhs_type {
                    Type::Tuple(types) => types.clone(),
                    _ => {
                        return Err(SemanticError::Other(format!(
                            "Cannot destructure non-tuple type '{}' with tuple pattern",
                            rhs_type.to_string()
                        )));
                    }
                };

                if sub_patterns.len() != element_types.len() {
                    return Err(SemanticError::Other(format!(
                        "Tuple destructure: expected {} elements, found {} patterns",
                        element_types.len(),
                        sub_patterns.len()
                    )));
                }

                let tuple_index_id = self.add_string_constant("tuple.index");

                for (idx, sub_pat) in sub_patterns.iter().enumerate() {
                    if !sub_pat.is_wildcard()
                        && let Pattern::Identifier(id) = sub_pat
                    {
                        self.builder.dup();
                        self.builder.ldi(idx as i64);
                        self.builder.call_host_function(tuple_index_id);
                        let elem_ty = element_types[idx].clone();
                        let var_id = self
                            .local_scope
                            .as_mut()
                            .unwrap()
                            .add_variable(id.name.clone(), elem_ty, is_mutable)
                            .map_err(|_| SemanticError::VariableOutsideFunction {
                                name: id.name.to_string(),
                                pos: id.pos(),
                            })?;
                        self.builder.stvar(var_id);
                    }
                }

                // Pop the tuple value from the stack
                self.builder.pop();

                Ok(Type::Primitive(PrimitiveType::Unit))
            }

            _ => Err(SemanticError::Other(
                "Unsupported pattern in let destructuring".to_string(),
            )),
        }
    }

    fn generate_function_decl(&mut self, declaration: FunctionDeclaration) -> SaResult<Type> {
        self.process_function_declaration(declaration)?;
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_throw(&mut self, expression: Expression) -> SaResult<Type> {
        self.generate_expression(expression)?;
        self.builder.throw();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_enum_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        values: Vec<EnumVariant>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let type_id = self.register_enum_decl(
            is_pub,
            name.clone(),
            attributes.clone(),
            conformance.clone(),
            type_parameters.clone(),
        )?;
        self.resolve_enum_decl(
            type_id,
            is_pub,
            name,
            attributes,
            conformance,
            type_parameters,
            values,
            methods,
        )?;
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn register_enum_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        _conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
    ) -> SaResult<TypeId> {
        let qualified_name = self.symbol_table.get_new_symbol_qualified_name(&name.name);

        let type_param_names: Vec<InternedString> = type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let type_param_bounds_list: Vec<Vec<InternedString>> = type_parameters
            .iter()
            .map(|tp| tp.bounds.iter().map(|b| b.name.clone()).collect())
            .collect();

        let ty = Enum {
            qualified_name: qualified_name.clone(),
            variants: Vec::new(),
            attributes: attributes.clone(),
            type_parameters: type_param_names,
            type_param_bounds: type_param_bounds_list,
            generic_variant_types: if type_parameters.is_empty() {
                None
            } else {
                Some(Vec::new())
            },
            monomorphization: None,
            conforming_to: Vec::new(),
            methods: Vec::new(),
            source_module: None,
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Enum(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );
        self.symbol_table.push_symbol(symbol);
        self.symbol_table.pop_symbol();

        let _ = is_pub;
        Ok(type_id)
    }

    pub(super) fn resolve_enum_decl(
        &mut self,
        type_id: TypeId,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        values: Vec<EnumVariant>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let symbol_id = self
            .symbol_table
            .find_symbol_for_type(type_id)
            .ok_or_else(|| {
                SemanticError::Other(format!("Type symbol not found for enum '{}'", name.name))
            })?;
        self.symbol_table.push_existing_symbol(symbol_id);

        let qualified_name = match self.symbol_table.get_type(type_id) {
            TypeDefinition::Enum(e) => e.qualified_name.clone(),
            _ => {
                self.symbol_table.pop_symbol();
                return Err(SemanticError::Other(format!(
                    "Type id {} is not an enum",
                    type_id
                )));
            }
        };
        let is_generic = !type_parameters.is_empty();

        let (variants, generic_variant_types) = if is_generic {
            let generic_types: Vec<Vec<crate::ast::Type>> =
                values.iter().map(|v| v.fields.clone()).collect();
            let variants: Vec<(InternedString, Vec<Type>)> =
                values.iter().map(|v| (v.name.clone(), vec![])).collect();
            (variants, Some(generic_types))
        } else {
            let mut resolved_variants = Vec::new();
            for variant in values {
                let mut field_types = Vec::new();
                for field_type in &variant.fields {
                    let resolved_type = self.get_semantic_type(field_type)?;
                    field_types.push(resolved_type);
                }
                resolved_variants.push((variant.name.clone(), field_types));
            }
            (resolved_variants, None)
        };

        let conforming_to = self.resolve_conformances(&conformance)?;

        let type_param_names: Vec<InternedString> = type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let type_param_bounds_list: Vec<Vec<InternedString>> = type_parameters
            .iter()
            .map(|tp| tp.bounds.iter().map(|b| b.name.clone()).collect())
            .collect();

        let ty = Enum {
            qualified_name: qualified_name.clone(),
            variants,
            attributes: attributes.clone(),
            type_parameters: type_param_names.clone(),
            type_param_bounds: type_param_bounds_list.clone(),
            generic_variant_types,
            monomorphization: None,
            conforming_to: conforming_to.clone(),
            methods: Vec::new(),
            source_module: None,
        };
        self.symbol_table
            .update_type(type_id, TypeDefinition::Enum(ty));

        let prev_enclosing_type_params =
            std::mem::replace(&mut self.enclosing_type_params, type_param_names);
        let prev_enclosing_type_param_bounds = std::mem::replace(
            &mut self.enclosing_type_param_bounds,
            type_param_bounds_list,
        );

        let mut method_decls = Vec::new();
        for method in methods {
            self.register_function_signature(&method.function)?;
            method_decls.push(method.function);
        }
        for decl in method_decls {
            self.generate_function_body(decl)?;
        }

        self.enclosing_type_params = prev_enclosing_type_params;
        self.enclosing_type_param_bounds = prev_enclosing_type_param_bounds;

        self.check_struct_conformance(type_id, &conforming_to, name.line, name.col)?;

        let _ = is_pub;

        self.symbol_table.pop_symbol();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_struct_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        fields: Vec<StructField>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let type_id = self.register_struct_decl(
            is_pub,
            name.clone(),
            attributes.clone(),
            conformance.clone(),
            type_parameters.clone(),
        )?;
        self.resolve_struct_decl(
            type_id,
            is_pub,
            name,
            attributes,
            conformance,
            type_parameters,
            fields,
            methods,
        )?;
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn register_struct_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        _conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
    ) -> SaResult<TypeId> {
        let qualified_name = self.symbol_table.get_new_symbol_qualified_name(&name.name);

        let type_param_names: Vec<InternedString> = type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let type_param_bounds_list: Vec<Vec<InternedString>> = type_parameters
            .iter()
            .map(|tp| tp.bounds.iter().map(|b| b.name.clone()).collect())
            .collect();

        let ty = Struct {
            qualified_name: qualified_name.clone(),
            fields: Vec::new(),
            field_defaults: Vec::new(),
            attributes: attributes.clone(),
            type_parameters: type_param_names,
            type_param_bounds: type_param_bounds_list,
            generic_field_types: if type_parameters.is_empty() {
                None
            } else {
                Some(Vec::new())
            },
            monomorphization: None,
            conforming_to: Vec::new(),
            methods: Vec::new(),
            source_module: None,
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Struct(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );
        self.symbol_table.push_symbol(symbol);
        self.symbol_table.pop_symbol();

        let _ = is_pub;
        Ok(type_id)
    }

    pub(super) fn resolve_struct_decl(
        &mut self,
        type_id: TypeId,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        fields: Vec<StructField>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let symbol_id = self
            .symbol_table
            .find_symbol_for_type(type_id)
            .ok_or_else(|| {
                SemanticError::Other(format!("Type symbol not found for struct '{}'", name.name))
            })?;
        self.symbol_table.push_existing_symbol(symbol_id);

        let qualified_name = match self.symbol_table.get_type(type_id) {
            TypeDefinition::Struct(s) => s.qualified_name.clone(),
            _ => {
                self.symbol_table.pop_symbol();
                return Err(SemanticError::Other(format!(
                    "Type id {} is not a struct",
                    type_id
                )));
            }
        };

        let type_param_names: Vec<InternedString> = type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let type_param_bounds_list: Vec<Vec<InternedString>> = type_parameters
            .iter()
            .map(|tp| tp.bounds.iter().map(|b| b.name.clone()).collect())
            .collect();

        let is_generic = !type_param_names.is_empty();

        let (fields_with_types, generic_field_types) = if is_generic {
            let parsed_field_types: Vec<crate::ast::Type> =
                fields.iter().map(|f| f.field_type.clone()).collect();

            let placeholder_fields: Vec<(InternedString, Type)> = fields
                .iter()
                .enumerate()
                .map(|(idx, f)| {
                    let field_name = f
                        .name
                        .as_ref()
                        .map(|n| n.name.clone())
                        .unwrap_or_else(|| InternedString::from(idx.to_string()));
                    (field_name, Type::Primitive(PrimitiveType::Unit))
                })
                .collect();

            (placeholder_fields, Some(parsed_field_types))
        } else {
            let mut resolved_fields = Vec::new();
            for (idx, f) in fields.iter().enumerate() {
                let field_type = self.get_semantic_type(&f.field_type)?;
                let field_name = f
                    .name
                    .as_ref()
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| InternedString::from(idx.to_string()));
                resolved_fields.push((field_name, field_type));
            }
            (resolved_fields, None)
        };

        let field_defaults = fields.iter().map(|f| f.default_value.clone()).collect();
        let conforming_to = self.resolve_conformances(&conformance)?;

        let ty = Struct {
            qualified_name: qualified_name.clone(),
            fields: fields_with_types,
            field_defaults,
            attributes: attributes.clone(),
            type_parameters: type_param_names.clone(),
            type_param_bounds: type_param_bounds_list.clone(),
            generic_field_types,
            monomorphization: None,
            conforming_to: conforming_to.clone(),
            methods: Vec::new(),
            source_module: None,
        };
        self.symbol_table
            .update_type(type_id, TypeDefinition::Struct(ty));

        let prev_enclosing_type_params =
            std::mem::replace(&mut self.enclosing_type_params, type_param_names);
        let prev_enclosing_type_param_bounds = std::mem::replace(
            &mut self.enclosing_type_param_bounds,
            type_param_bounds_list,
        );

        let mut method_decls = Vec::new();
        for method in methods {
            self.register_function_signature(&method.function)?;
            method_decls.push(method.function);
        }
        for decl in method_decls {
            self.generate_function_body(decl)?;
        }

        self.enclosing_type_params = prev_enclosing_type_params;
        self.enclosing_type_param_bounds = prev_enclosing_type_param_bounds;

        self.check_struct_conformance(type_id, &conforming_to, name.line, name.col)?;

        let _ = is_pub;

        self.symbol_table.pop_symbol();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_proto_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        methods: Vec<ProtoMethod>,
    ) -> SaResult<Type> {
        let type_id = self.register_proto_decl(is_pub, name.clone(), attributes.clone())?;
        self.resolve_proto_decl(type_id, is_pub, name, attributes, methods)?;
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn register_proto_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
    ) -> SaResult<TypeId> {
        let qualified_name = self.symbol_table.get_new_symbol_qualified_name(&name.name);
        let ty = Proto {
            qualified_name: qualified_name.clone(),
            methods: vec![],
            attributes: attributes.clone(),
            foreign_type_tag: None,
            foreign_uuid: None,
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Proto(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );
        self.symbol_table.push_symbol(symbol);
        self.symbol_table.pop_symbol();

        let _ = is_pub;
        Ok(type_id)
    }

    pub(super) fn resolve_proto_decl(
        &mut self,
        type_id: TypeId,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        methods: Vec<ProtoMethod>,
    ) -> SaResult<Type> {
        let symbol_id = self
            .symbol_table
            .find_symbol_for_type(type_id)
            .ok_or_else(|| {
                SemanticError::Other(format!("Type symbol not found for proto '{}'", name.name))
            })?;
        self.symbol_table.push_existing_symbol(symbol_id);

        let qualified_name = match self.symbol_table.get_type(type_id) {
            TypeDefinition::Proto(p) => p.qualified_name.clone(),
            _ => {
                self.symbol_table.pop_symbol();
                return Err(SemanticError::Other(format!(
                    "Type id {} is not a proto",
                    type_id
                )));
            }
        };

        let mut methods_with_types = Vec::new();
        for m in &methods {
            let mut params = Vec::new();
            for p in &m.parameters {
                params.push(self.get_semantic_type(&p.arg_type)?);
            }
            let return_type = match &m.return_type {
                Some(t) => Some(self.get_semantic_type(t)?),
                None => None,
            };
            methods_with_types.push((m.name.name.clone(), params, return_type));
        }

        let foreign_attrs = Self::extract_foreign_attrs(&attributes);
        let ty = Proto {
            qualified_name: qualified_name.clone(),
            methods: methods_with_types.clone(),
            attributes: attributes.clone(),
            foreign_type_tag: foreign_attrs.as_ref().and_then(|f| f.type_tag.clone()),
            foreign_uuid: foreign_attrs.as_ref().and_then(|f| f.uuid.clone()),
        };
        self.symbol_table
            .update_type(type_id, TypeDefinition::Proto(ty));

        self.symbol_table.pop_symbol();

        if let Some(foreign) = foreign_attrs {
            self.synthesize_foreign_carrier(
                &name,
                &qualified_name,
                type_id,
                foreign,
                &methods_with_types,
            )?;
        }

        let _ = is_pub;

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    /// Synthesise the hidden `__ForeignCarrier_<F>` struct for an `@foreign`
    /// proto `F`. The carrier:
    ///   - has no fields,
    ///   - conforms to `F`,
    ///   - exposes one child symbol per proto method, with kind
    ///     `SymbolKind::HostMethod`.
    ///
    /// The runtime's `build_vtable` walks `Struct.conforming_to` and reads
    /// child symbols by method name, so the carrier's vtable entries are
    /// populated automatically without any new dispatch code at registration
    /// time.
    fn synthesize_foreign_carrier(
        &mut self,
        proto_ident: &Identifier,
        proto_qualified_name: &InternedString,
        proto_type_id: crate::semantic::TypeId,
        foreign: super::helpers::ForeignAttrs,
        methods: &[(InternedString, Vec<Type>, Option<Type>)],
    ) -> SaResult<()> {
        // Validate required keys.
        let dispatcher = foreign.dispatcher.ok_or_else(|| {
            SemanticError::Other(format!(
                "@foreign proto '{}' is missing required key 'dispatcher' at line {} column {}",
                proto_ident.name, proto_ident.line, proto_ident.col,
            ))
        })?;
        let type_tag = foreign.type_tag.ok_or_else(|| {
            SemanticError::Other(format!(
                "@foreign proto '{}' is missing required key 'type_tag' at line {} column {}",
                proto_ident.name, proto_ident.line, proto_ident.col,
            ))
        })?;

        // Enforce type_tag uniqueness across the module.
        if let Some((prev_line, prev_col)) = self.seen_foreign_type_tags.get(&type_tag).copied() {
            return Err(SemanticError::Other(format!(
                "@foreign type_tag '{}' on proto '{}' at line {} column {} is already claimed by a previous proto at line {} column {}",
                type_tag, proto_ident.name, proto_ident.line, proto_ident.col, prev_line, prev_col,
            )));
        }
        self.seen_foreign_type_tags
            .insert(type_tag.clone(), (proto_ident.line, proto_ident.col));

        // Allocate the carrier struct's qualified name and TypeId. The
        // double-underscore prefix is a soft convention: user code is
        // unlikely to type that identifier.
        let carrier_local_name =
            InternedString::from(format!("__ForeignCarrier_{}", proto_ident.name));
        let carrier_qualified_name = InternedString::from(format!(
            "{}__ForeignCarrier",
            // The proto's qualified_name is "<scope>.<protoname>"; replace
            // the trailing proto name with the carrier convention to keep
            // qualified-name uniqueness without depending on lexical scope.
            proto_qualified_name
                .as_str()
                .strip_suffix(proto_ident.name.as_str())
                .unwrap_or(proto_qualified_name.as_str()),
        ));

        let carrier_struct = Struct {
            qualified_name: carrier_qualified_name.clone(),
            fields: Vec::new(),
            field_defaults: Vec::new(),
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            type_param_bounds: Vec::new(),
            generic_field_types: None,
            monomorphization: None,
            conforming_to: vec![proto_type_id],
            methods: Vec::new(),
            source_module: None,
        };
        let carrier_type_id = self
            .symbol_table
            .add_type(TypeDefinition::Struct(carrier_struct));

        // Push the carrier symbol so that subsequent insert_symbol calls
        // attach as its children.
        let carrier_symbol = Symbol::new(
            carrier_local_name.clone(),
            carrier_qualified_name.clone(),
            SymbolKind::Type(carrier_type_id),
        );
        self.symbol_table.push_symbol(carrier_symbol);

        // For each proto method, insert a HostMethod child symbol on the
        // carrier. `build_vtable` will pick these up by name.
        for (idx, (method_name, params, return_type)) in methods.iter().enumerate() {
            let return_kind = match return_type {
                None => crate::semantic::ReturnKind::Void,
                Some(Type::Nullable(_)) => crate::semantic::ReturnKind::Optional,
                Some(_) => crate::semantic::ReturnKind::Value,
            };
            let qualified_method_name =
                InternedString::from(format!("{}.{}", carrier_qualified_name, method_name,));
            let host_method_symbol = Symbol::new(
                method_name.clone(),
                qualified_method_name,
                SymbolKind::HostMethod {
                    dispatcher_name: dispatcher.clone(),
                    method_name: method_name.clone(),
                    type_tag: type_tag.clone(),
                    param_count: params.len() as u32,
                    return_kind,
                    vtable_slot: idx as u32,
                    return_ty: map_host_return_ty(return_type, &self.symbol_table),
                },
            );
            self.symbol_table.insert_symbol(host_method_symbol);
        }

        self.symbol_table.pop_symbol();

        // Suppress unused warning until Phase 3 wires the finalizer.
        let _ = foreign.finalizer;

        Ok(())
    }

    fn generate_return(
        &mut self,
        expression: Option<Expression>,
        pos: SourcePos,
    ) -> SaResult<Type> {
        let expected =
            self.enclosing_return_types.last().cloned().ok_or_else(|| {
                SemanticError::Other("`return` outside of a function".to_string())
            })?;

        match expression {
            None => {
                if expected != Type::Primitive(PrimitiveType::Unit) {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("declared return type {}", self.type_to_string(&expected)),
                        rhs: "no return value (`return;`)".to_string(),
                        pos: Some(pos),
                    });
                }
                self.builder.ldi(0);
            }
            Some(expression) => {
                // Check if this expression involves a move (before consuming it)
                let move_info: Option<(u32, bool)> =
                    if let Expression::Identifier(ref ident) = expression {
                        if let Some(var_id) = self
                            .local_scope
                            .as_ref()
                            .unwrap()
                            .find_variable(&ident.name)
                        {
                            let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                            if !ty.is_copy_treated(&self.symbol_table) {
                                Some((var_id, false))
                            } else {
                                None
                            }
                        } else if let Some(param_id) =
                            self.local_scope.as_ref().unwrap().find_param(&ident.name)
                        {
                            let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                            if !ty.is_copy_treated(&self.symbol_table) {
                                Some((param_id, true))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                let ty = self.generate_expression(expression)?;

                // Mark variable as moved if needed
                if let Some((id, is_param)) = move_info {
                    if is_param {
                        self.mark_param_moved(id);
                    } else {
                        self.mark_variable_moved(id);
                    }
                }

                if matches!(ty, Type::Reference(_)) {
                    return Err(SemanticError::Other(
                        "Cannot return a non-escapable ref value".to_string(),
                    ));
                }

                if expected != ty {
                    return Err(SemanticError::TypeMismatch {
                        lhs: format!("declared return type {}", self.type_to_string(&expected)),
                        rhs: format!("returned value of type {}", self.type_to_string(&ty)),
                        pos: Some(pos),
                    });
                }
            }
        }

        self.builder.ret();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    pub(super) fn generate_import(
        &mut self,
        module_path: InternedString,
        alias: Option<InternedString>,
    ) -> SaResult<Type> {
        // Import semantics: try module first, then symbol from parent module
        let segments: Vec<&str> = module_path.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return Err(SemanticError::ImportError {
                module_path: module_path.to_string(),
                pos: SourcePos {
                    line: 0,
                    col: 0,
                    module: Some(self._current_module_path.to_string()),
                },
            });
        }

        // Binding name: alias or last segment
        let binding_name: InternedString = alias
            .clone()
            .unwrap_or_else(|| InternedString::from(segments.last().unwrap().to_string()));

        // 1) Try module import: load full module_path
        if let Some(source) = self.module_provider.load_module(&module_path) {
            if !self.imported_modules.contains_key(&module_path) {
                let imported_module = self.compile_module(&module_path, source)?;
                self.imported_modules
                    .insert(module_path.clone(), imported_module);
            }

            let module_id = self.symbol_table.register_module(module_path.clone(), 0);
            let module_symbol = Symbol::new(
                binding_name.clone(),
                module_path.clone(),
                SymbolKind::Module(module_id),
            );
            self.symbol_table.insert_symbol(module_symbol);
            return Ok(Type::Primitive(PrimitiveType::Unit));
        }

        // 2) Fallback: treat last segment as symbol name in parent module
        if segments.len() < 2 {
            return Err(SemanticError::ImportError {
                module_path: module_path.to_string(),
                pos: SourcePos {
                    line: 0,
                    col: 0,
                    module: Some(self._current_module_path.to_string()),
                },
            });
        }

        let parent_path = segments[..segments.len() - 1].join(".");
        let symbol_name = segments.last().unwrap().to_string();

        let parent_source = self
            .module_provider
            .load_module(&parent_path)
            .ok_or_else(|| SemanticError::ImportError {
                module_path: parent_path.clone(),
                pos: SourcePos {
                    line: 0,
                    col: 0,
                    module: Some(self._current_module_path.to_string()),
                },
            })?;

        if !self.imported_modules.contains_key(parent_path.as_str()) {
            let imported_module = self.compile_module(&parent_path, parent_source)?;
            self.imported_modules
                .insert(InternedString::from(parent_path.clone()), imported_module);
        }

        let imported_parent = self
            .imported_modules
            .get(parent_path.as_str())
            .cloned()
            .ok_or_else(|| {
                SemanticError::Other(format!("Invalid module import: {}", parent_path))
            })?;
        // Symbols exported from parent: children of root
        let root = imported_parent
            .symbols
            .first()
            .ok_or_else(|| SemanticError::Other(format!("Invalid module root: {}", parent_path)))?;
        if let Some(sym_id) = root.children.get(symbol_name.as_str())
            && let Some(sym) = imported_parent.symbols.get(*sym_id as usize)
        {
            let resolved_kind = match sym.kind {
                SymbolKind::Type(imported_type_id) => {
                    if let Some(existing_symbol) = self
                        .symbol_table
                        .find_symbol_by_qualified_name(&sym.qualified_name)
                    {
                        if let SymbolKind::Type(existing_type_id) = existing_symbol.kind {
                            SymbolKind::Type(existing_type_id)
                        } else {
                            sym.kind.clone()
                        }
                    } else {
                        let mut type_map = std::collections::HashMap::new();
                        let new_type_id = self.import_type_from_module(
                            &imported_parent,
                            imported_type_id,
                            &mut type_map,
                        )?;
                        SymbolKind::Type(new_type_id)
                    }
                }
                _ => sym.kind.clone(),
            };

            let new_symbol = Symbol::new(
                binding_name.clone(),
                sym.qualified_name.clone(),
                resolved_kind,
            );
            self.symbol_table.insert_symbol(new_symbol);
            return Ok(Type::Primitive(PrimitiveType::Unit));
        }

        Err(SemanticError::Other(format!(
            "Symbol '{}' not found in module '{}'",
            symbol_name, parent_path
        )))
    }

    /// Helper to resolve protocol conformances from identifiers
    fn resolve_conformances(&mut self, conformance: &[Identifier]) -> SaResult<Vec<u32>> {
        let mut conforming_to = Vec::new();
        for proto_name in conformance {
            let proto_type_id = self.resolve_proto_identifier(proto_name)?;
            conforming_to.push(proto_type_id);
        }
        Ok(conforming_to)
    }

    pub(super) fn import_type_from_module(
        &mut self,
        module: &Module,
        type_id: u32,
        type_map: &mut std::collections::HashMap<u32, u32>,
    ) -> SaResult<u32> {
        if let Some(&mapped_id) = type_map.get(&type_id) {
            return Ok(mapped_id);
        }

        // Derive source module path from the module's root symbol
        let source_module_path = module
            .symbols
            .first()
            .map(|root| root.qualified_name.clone());

        let type_def = module.types.get(type_id as usize).ok_or_else(|| {
            SemanticError::Other(format!("Type id {} not found in imported module", type_id))
        })?;

        // Check if a type with the same qualified_name already exists in the
        // current symbol table.  This ensures that multiple imports of the same
        // type (e.g. via a return type mapping and a qualified type annotation)
        // share a single TypeId.
        let qualified_name = match type_def {
            TypeDefinition::Struct(s) => &s.qualified_name,
            TypeDefinition::Enum(e) => &e.qualified_name,
            TypeDefinition::Proto(p) => &p.qualified_name,
        };
        if let Some(existing_symbol) = self
            .symbol_table
            .find_symbol_by_qualified_name(qualified_name)
            && let SymbolKind::Type(existing_type_id) = existing_symbol.kind
        {
            type_map.insert(type_id, existing_type_id);
            return Ok(existing_type_id);
        }

        let mapped_def = match type_def {
            TypeDefinition::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        let mapped_ty = self.map_type_from_module(module, ty, type_map)?;
                        Ok((name.clone(), mapped_ty))
                    })
                    .collect::<SaResult<Vec<_>>>()?;

                let conforming_to = s
                    .conforming_to
                    .iter()
                    .map(|proto_id| self.import_type_from_module(module, *proto_id, type_map))
                    .collect::<SaResult<Vec<_>>>()?;

                TypeDefinition::Struct(Struct {
                    qualified_name: s.qualified_name.clone(),
                    fields,
                    field_defaults: s.field_defaults.clone(),
                    attributes: s.attributes.clone(),
                    type_parameters: s.type_parameters.clone(),
                    type_param_bounds: s.type_param_bounds.clone(),
                    generic_field_types: s.generic_field_types.clone(),
                    monomorphization: s.monomorphization.clone(),
                    conforming_to,
                    methods: Vec::new(),
                    source_module: source_module_path.clone(),
                })
            }
            TypeDefinition::Enum(e) => {
                let variants = e
                    .variants
                    .iter()
                    .map(|(name, fields)| {
                        let mapped_fields = fields
                            .iter()
                            .map(|field_ty| self.map_type_from_module(module, field_ty, type_map))
                            .collect::<SaResult<Vec<_>>>()?;
                        Ok((name.clone(), mapped_fields))
                    })
                    .collect::<SaResult<Vec<_>>>()?;

                let conforming_to = e
                    .conforming_to
                    .iter()
                    .map(|proto_id| self.import_type_from_module(module, *proto_id, type_map))
                    .collect::<SaResult<Vec<_>>>()?;

                TypeDefinition::Enum(Enum {
                    qualified_name: e.qualified_name.clone(),
                    variants,
                    attributes: e.attributes.clone(),
                    type_parameters: e.type_parameters.clone(),
                    type_param_bounds: e.type_param_bounds.clone(),
                    generic_variant_types: e.generic_variant_types.clone(),
                    monomorphization: e.monomorphization.clone(),
                    conforming_to,
                    methods: Vec::new(),
                    source_module: source_module_path.clone(),
                })
            }
            TypeDefinition::Proto(p) => {
                let methods = p
                    .methods
                    .iter()
                    .map(|(name, params, return_type)| {
                        let mapped_params = params
                            .iter()
                            .map(|param| self.map_type_from_module(module, param, type_map))
                            .collect::<SaResult<Vec<_>>>()?;
                        let mapped_return = match return_type {
                            Some(ret) => Some(self.map_type_from_module(module, ret, type_map)?),
                            None => None,
                        };
                        Ok((name.clone(), mapped_params, mapped_return))
                    })
                    .collect::<SaResult<Vec<_>>>()?;

                TypeDefinition::Proto(Proto {
                    qualified_name: p.qualified_name.clone(),
                    methods,
                    attributes: p.attributes.clone(),
                    foreign_type_tag: p.foreign_type_tag.clone(),
                    foreign_uuid: p.foreign_uuid.clone(),
                })
            }
        };

        let qualified_name_owned = qualified_name.clone();
        let new_id = self.symbol_table.add_type(mapped_def);
        type_map.insert(type_id, new_id);

        // Insert a symbol so find_symbol_by_qualified_name can locate this type
        // during monomorphization (type_to_parsed_type emits the qualified name).
        let new_symbol = crate::semantic::Symbol::new(
            qualified_name_owned.clone(),
            qualified_name_owned,
            SymbolKind::Type(new_id),
        );
        self.symbol_table.insert_symbol(new_symbol);

        Ok(new_id)
    }

    pub(super) fn map_type_from_module(
        &mut self,
        module: &Module,
        ty: &Type,
        type_map: &mut std::collections::HashMap<u32, u32>,
    ) -> SaResult<Type> {
        let mapped = match ty {
            Type::Primitive(p) => Type::Primitive(*p),
            Type::Reference(inner) => Type::Reference(Box::new(
                self.map_type_from_module(module, inner, type_map)?,
            )),
            Type::MutableReference(inner) => Type::MutableReference(Box::new(
                self.map_type_from_module(module, inner, type_map)?,
            )),
            Type::Array(inner) => Type::Array(Box::new(
                self.map_type_from_module(module, inner, type_map)?,
            )),
            Type::BoxType(inner) => Type::BoxType(Box::new(
                self.map_type_from_module(module, inner, type_map)?,
            )),
            Type::Nullable(inner) => Type::Nullable(Box::new(
                self.map_type_from_module(module, inner, type_map)?,
            )),
            Type::Struct(id) => {
                let new_id = self.import_type_from_module(module, *id, type_map)?;
                Type::Struct(new_id)
            }
            Type::Enum(id) => {
                let new_id = self.import_type_from_module(module, *id, type_map)?;
                Type::Enum(new_id)
            }
            Type::Proto(id) => {
                let new_id = self.import_type_from_module(module, *id, type_map)?;
                Type::Proto(new_id)
            }
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.map_type_from_module(module, p, type_map))
                    .collect::<SaResult<Vec<_>>>()?,
                return_type: Box::new(self.map_type_from_module(module, return_type, type_map)?),
            },
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|t| self.map_type_from_module(module, t, type_map))
                    .collect::<SaResult<Vec<_>>>()?,
            ),
            Type::Map(k, v) => Type::Map(
                Box::new(self.map_type_from_module(module, k, type_map)?),
                Box::new(self.map_type_from_module(module, v, type_map)?),
            ),
        };

        Ok(mapped)
    }
}

fn map_host_return_ty(
    return_type: &Option<Type>,
    symbol_table: &crate::semantic::SymbolTable,
) -> crate::semantic::HostReturnTy {
    use crate::semantic::HostReturnTy as H;

    fn map_ty(ty: &Type, symbol_table: &crate::semantic::SymbolTable) -> H {
        match ty {
            Type::Primitive(PrimitiveType::Unit) => H::Void,
            Type::Primitive(PrimitiveType::Int) => H::Int,
            Type::Primitive(PrimitiveType::Float) => H::Float,
            Type::Primitive(PrimitiveType::String) => H::String,
            Type::Nullable(inner) => H::Optional(Box::new(map_ty(inner, symbol_table))),
            Type::Array(inner) => H::Array(Box::new(map_ty(inner, symbol_table))),
            Type::Proto(type_id) => {
                if let Some(TypeDefinition::Proto(proto)) =
                    symbol_table.types.get(*type_id as usize)
                    && let Some(type_tag) = &proto.foreign_type_tag
                {
                    H::Foreign {
                        type_tag: type_tag.clone(),
                    }
                } else {
                    H::Void
                }
            }
            _ => H::Void,
        }
    }

    match return_type {
        Some(ty) => map_ty(ty, symbol_table),
        None => H::Void,
    }
}
