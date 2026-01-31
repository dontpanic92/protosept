use crate::ast::{
    Attribute, EnumVariant, FunctionDeclaration, Identifier, ProtoMethod, StructField,
    StructMethod, TypeParameter,
};
use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::{
    ast::{Expression, Statement},
    semantic::{Enum, PrimitiveType, Proto, Struct, Symbol, SymbolKind, Type, TypeDefinition},
};

use super::{Generator, SaResult};

impl Generator {
    pub(super) fn generate_statement(&mut self, statement: Statement) -> SaResult<Type> {
        match statement {
            Statement::Let {
                is_mutable,
                identifier,
                type_annotation,
                expression,
            } => self.generate_let(is_mutable, identifier, type_annotation, expression),
            Statement::Expression(expression) => self.generate_expression(expression),
            Statement::FunctionDeclaration(declaration) => {
                self.generate_function_decl(declaration)
            }
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
            Statement::Return(expression) => self.generate_return(*expression),
            Statement::Import { module_path, alias } => self.generate_import(module_path, alias),
        }
    }

    fn generate_let(
        &mut self,
        is_mutable: bool,
        identifier: Identifier,
        type_annotation: Option<crate::ast::Type>,
        expression: Expression,
    ) -> SaResult<Type> {
        // Check if this expression involves a move (before consuming it)
        let move_info = self.compute_move_info(&expression);

        let inferred_ty = self.generate_expression(expression)?;

        // Mark variable as moved if needed
        if let Some(var_id) = move_info {
            self.mark_variable_moved(var_id);
        }

        // Validate type annotation if provided
        let final_ty = if let Some(annotation) = type_annotation {
            let annotated_ty = self.get_semantic_type(&annotation)?;

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
                name: identifier.name.clone(),
                pos: identifier.pos(),
            })?;

        self.builder.stvar(var_id);
        Ok(Type::Primitive(PrimitiveType::Unit))
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

    fn generate_enum_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        values: Vec<EnumVariant>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(name.name.clone());

        // Check if this is a generic enum
        let is_generic = !type_parameters.is_empty();

        let (variants, generic_variant_types) = if is_generic {
            // For generic enums, store the original AST types
            let generic_types: Vec<Vec<crate::ast::Type>> =
                values.iter().map(|v| v.fields.clone()).collect();
            // Don't resolve types yet - will be done during monomorphization
            let variants: Vec<(String, Vec<Type>)> =
                values.iter().map(|v| (v.name.clone(), vec![])).collect();
            (variants, Some(generic_types))
        } else {
            // For non-generic enums, resolve field types now
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

        // Resolve protocol conformances
        let conforming_to = self.resolve_conformances(&conformance)?;

        let ty = Enum {
            qualified_name: qualified_name.clone(),
            variants,
            attributes: attributes.clone(),
            type_parameters: type_parameters
                .iter()
                .map(|tp| tp.name.name.clone())
                .collect(),
            generic_variant_types,
            monomorphization: None,
            conforming_to: conforming_to.clone(),
            methods: Vec::new(),
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Enum(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );

        self.symbol_table.push_symbol(symbol);

        // Process enum methods
        for method in methods {
            self.process_function_declaration(method.function)?;
        }

        // Check conformance after processing methods
        self.check_struct_conformance(type_id, &conforming_to, name.line, name.col)?;

        // TODO: Store is_pub for module visibility checking
        let _ = is_pub;

        self.symbol_table.pop_symbol();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_struct_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>,
        type_parameters: Vec<TypeParameter>,
        fields: Vec<StructField>,
        methods: Vec<StructMethod>,
    ) -> SaResult<Type> {
        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(name.name.clone());

        // Extract type parameter names
        let type_param_names: Vec<String> = type_parameters
            .iter()
            .map(|tp| tp.name.name.clone())
            .collect();

        let is_generic = !type_param_names.is_empty();

        // For generic structs, store parsed field types; for non-generic, resolve them
        let (fields_with_types, generic_field_types) = if is_generic {
            // For generic structs, store placeholder types and keep parsed types
            let parsed_field_types: Vec<crate::ast::Type> =
                fields.iter().map(|f| f.field_type.clone()).collect();

            // Use Unit as placeholder - these will be properly typed during monomorphization
            let placeholder_fields: Vec<(String, Type)> = fields
                .iter()
                .enumerate()
                .map(|(idx, f)| {
                    let field_name = f
                        .name
                        .as_ref()
                        .map(|n| n.name.clone())
                        .unwrap_or_else(|| idx.to_string());
                    (field_name, Type::Primitive(PrimitiveType::Unit))
                })
                .collect();

            (placeholder_fields, Some(parsed_field_types))
        } else {
            // For non-generic structs, resolve types normally
            let mut resolved_fields = Vec::new();
            for (idx, f) in fields.iter().enumerate() {
                let field_type = self.get_semantic_type(&f.field_type)?;
                let field_name = f
                    .name
                    .as_ref()
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| idx.to_string());
                resolved_fields.push((field_name, field_type));
            }
            (resolved_fields, None)
        };

        let field_defaults = fields.iter().map(|f| f.default_value.clone()).collect();

        // Resolve protocol conformances
        let conforming_to = self.resolve_conformances(&conformance)?;

        let ty = Struct {
            qualified_name: qualified_name.clone(),
            fields: fields_with_types,
            field_defaults,
            attributes: attributes.clone(),
            type_parameters: type_param_names,
            generic_field_types,
            monomorphization: None, // This is the generic definition, not a monomorphization
            conforming_to: conforming_to.clone(),
            methods: Vec::new(),
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Struct(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );
        self.symbol_table.push_symbol(symbol);

        for method in methods {
            self.process_function_declaration(method.function)?;
        }

        // Check conformance after processing methods
        self.check_struct_conformance(type_id, &conforming_to, name.line, name.col)?;

        // TODO: Store is_pub for module visibility checking
        let _ = is_pub;

        self.symbol_table.pop_symbol();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_proto_decl(
        &mut self,
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        methods: Vec<ProtoMethod>,
    ) -> SaResult<Type> {
        let qualified_name = self
            .symbol_table
            .get_new_symbol_qualified_name(name.name.clone());

        // First add the proto to the symbol table as a forward declaration
        // so that method parameters can reference it
        let ty = Proto {
            qualified_name: qualified_name.clone(),
            methods: vec![],
            attributes: attributes.clone(),
        };
        let type_id = self.symbol_table.add_type(TypeDefinition::Proto(ty));

        let symbol = Symbol::new(
            name.name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(type_id),
        );
        self.symbol_table.push_symbol(symbol);

        // Now process the method signatures
        let mut methods_with_types = Vec::new();
        for m in methods {
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

        // Update the proto with the actual method signatures
        let ty = Proto {
            qualified_name: qualified_name.clone(),
            methods: methods_with_types,
            attributes: attributes.clone(),
        };
        self.symbol_table.types[type_id as usize] = TypeDefinition::Proto(ty);

        self.symbol_table.pop_symbol();

        // TODO: Store is_pub for module visibility checking
        let _ = is_pub;

        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_return(&mut self, expression: Expression) -> SaResult<Type> {
        // Check if this expression involves a move (before consuming it)
        let move_info = if let Expression::Identifier(ref ident) = expression {
            if let Some(var_id) = self
                .local_scope
                .as_ref()
                .unwrap()
                .find_variable(&ident.name)
            {
                let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                if !ty.is_copy_treated(&self.symbol_table) {
                    Some(var_id)
                } else {
                    None
                }
            } else if let Some(param_id) =
                self.local_scope.as_ref().unwrap().find_param(&ident.name)
            {
                let ty = self.local_scope.as_ref().unwrap().get_param_type(param_id);
                if !ty.is_copy_treated(&self.symbol_table) {
                    Some(param_id)
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
        if let Some(var_id) = move_info {
            self.mark_variable_moved(var_id);
        }

        if matches!(ty, Type::Reference(_)) {
            return Err(SemanticError::Other(
                "Cannot return a non-escapable ref value".to_string(),
            ));
        }
        self.builder.ret();
        Ok(Type::Primitive(PrimitiveType::Unit))
    }

    fn generate_import(&mut self, module_path: String, alias: Option<String>) -> SaResult<Type> {
        // Import semantics: try module first, then symbol from parent module
        let segments: Vec<&str> = module_path.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return Err(SemanticError::ImportError {
                module_path: module_path.clone(),
                pos: SourcePos { line: 0, col: 0 },
            });
        }

        // Binding name: alias or last segment
        let binding_name = alias
            .clone()
            .unwrap_or_else(|| segments.last().unwrap().to_string());

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
                module_path: module_path.clone(),
                pos: SourcePos { line: 0, col: 0 },
            });
        }

        let parent_path = segments[..segments.len() - 1].join(".");
        let symbol_name = segments.last().unwrap().to_string();

        let parent_source =
            self.module_provider
                .load_module(&parent_path)
                .ok_or_else(|| SemanticError::ImportError {
                    module_path: parent_path.clone(),
                    pos: SourcePos { line: 0, col: 0 },
                })?;

        if !self.imported_modules.contains_key(&parent_path) {
            let imported_module = self.compile_module(&parent_path, parent_source)?;
            self.imported_modules
                .insert(parent_path.clone(), imported_module);
        }

        let imported_parent = self.imported_modules.get(&parent_path).unwrap();
        // Symbols exported from parent: children of root
        let root = imported_parent.symbols.get(0).ok_or_else(|| {
            SemanticError::Other(format!("Invalid module root: {}", parent_path))
        })?;
        if let Some(sym_id) = root.children.get(&symbol_name) {
            if let Some(sym) = imported_parent.symbols.get(*sym_id as usize) {
                let new_symbol = Symbol::new(
                    binding_name.clone(),
                    sym.qualified_name.clone(),
                    sym.kind.clone(),
                );
                self.symbol_table.insert_symbol(new_symbol);
                return Ok(Type::Primitive(PrimitiveType::Unit));
            }
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
}
