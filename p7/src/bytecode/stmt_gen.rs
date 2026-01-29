use crate::errors::SourcePos;
use crate::{
    ast::{Expression, Statement},
    semantic::{Enum, PrimitiveType, Proto, Struct, Symbol, SymbolKind, Type, UserDefinedType},
};
use crate::errors::SemanticError;

use super::codegen::{Generator, SaResult};

impl Generator {
    pub(super) fn generate_statement(&mut self, statement: Statement) -> SaResult<Type> {
        match statement {
            Statement::Let {
                is_mutable,
                identifier,
                type_annotation,
                expression,
            } => {
                // Check if this expression involves a move (before consuming it)
                let move_info = if let Expression::Identifier(ref ident) = expression {
                    if let Some(var_id) = self.local_scope.as_ref().unwrap().find_variable(&ident.name) {
                        let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                        if !ty.is_copy_treated(&self.symbol_table) {
                            Some(var_id)
                        } else {
                            None
                        }
                    } else if let Some(param_id) = self.local_scope.as_ref().unwrap().find_param(&ident.name) {
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
                            pos: Some(SourcePos {
                                line: identifier.line,
                                col: identifier.col,
                            }),
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
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })?;

                self.builder.stvar(var_id);
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Expression(expression) => self.generate_expression(expression),
            Statement::FunctionDeclaration(declaration) => {
                self.process_function_declaration(declaration)?;

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Throw(expression) => {
                self.generate_expression(expression)?;
                self.builder.throw();
                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::EnumDeclaration {
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                values,
                methods,
            } => {
                let qualified_name = self
                    .symbol_table
                    .get_new_symbol_qualified_name(name.name.clone());
                
                // Check if this is a generic enum
                let is_generic = !type_parameters.is_empty();
                
                let (variants, generic_variant_types) = if is_generic {
                    // For generic enums, store the original AST types
                    let generic_types: Vec<Vec<crate::ast::Type>> = values.iter()
                        .map(|v| v.fields.clone())
                        .collect();
                    // Don't resolve types yet - will be done during monomorphization
                    let variants: Vec<(String, Vec<Type>)> = values.iter()
                        .map(|v| (v.name.clone(), vec![]))
                        .collect();
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
                let mut conforming_to = Vec::new();
                for proto_name in conformance {
                    let proto_type_id = self.resolve_proto_identifier(&proto_name)?;
                    conforming_to.push(proto_type_id);
                }
                
                let ty = Enum {
                    qualified_name: qualified_name.clone(),
                    variants,
                    attributes: attributes.clone(),
                    type_parameters: type_parameters.iter().map(|tp| tp.name.name.clone()).collect(),
                    generic_variant_types,
                    monomorphization: None,
                    conforming_to: conforming_to.clone(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Enum(ty));

                let symbol =
                    Symbol::new(name.name.clone(), qualified_name.clone(), SymbolKind::Enum(type_id));

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
            Statement::StructDeclaration {
                is_pub,
                name,
                attributes,
                conformance,
                type_parameters,
                fields,
                methods,
            } => {
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
                    let parsed_field_types: Vec<crate::ast::Type> = fields
                        .iter()
                        .map(|f| f.field_type.clone())
                        .collect();
                    
                    // Use Unit as placeholder - these will be properly typed during monomorphization
                    let placeholder_fields: Vec<(String, Type)> = fields
                        .iter()
                        .enumerate()
                        .map(|(idx, f)| {
                            let field_name = f.name.as_ref()
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
                        let field_name = f.name.as_ref()
                            .map(|n| n.name.clone())
                            .unwrap_or_else(|| idx.to_string());
                        resolved_fields.push((field_name, field_type));
                    }
                    (resolved_fields, None)
                };
                
                let field_defaults = fields.iter().map(|f| f.default_value.clone()).collect();
                
                // Resolve protocol conformances
                let mut conforming_to = Vec::new();
                for proto_name in conformance {
                    let proto_type_id = self.resolve_proto_identifier(&proto_name)?;
                    conforming_to.push(proto_type_id);
                }

                let ty = Struct {
                    qualified_name: qualified_name.clone(),
                    fields: fields_with_types,
                    field_defaults,
                    attributes: attributes.clone(),
                    type_parameters: type_param_names,
                    generic_field_types,
                    monomorphization: None,  // This is the generic definition, not a monomorphization
                    conforming_to: conforming_to.clone(),
                };
                let type_id = self.symbol_table.add_udt(UserDefinedType::Struct(ty));

                let symbol = Symbol::new(name.name.clone(), qualified_name.clone(), SymbolKind::Struct(type_id));
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
            Statement::ProtoDeclaration {
                is_pub,
                name,
                attributes,
                methods,
            } => {
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
                let type_id = self.symbol_table.add_udt(UserDefinedType::Proto(ty));
                
                let symbol = Symbol::new(name.name.clone(), qualified_name.clone(), SymbolKind::Proto(type_id));
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
                self.symbol_table.types[type_id as usize] = UserDefinedType::Proto(ty);
                
                self.symbol_table.pop_symbol();
                
                // TODO: Store is_pub for module visibility checking
                let _ = is_pub;

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
            Statement::Return(expression) => {
                // Check if this expression involves a move (before consuming it)
                let move_info = if let Expression::Identifier(ref ident) = *expression {
                    if let Some(var_id) = self.local_scope.as_ref().unwrap().find_variable(&ident.name) {
                        let ty = self.local_scope.as_ref().unwrap().get_variable_type(var_id);
                        if !ty.is_copy_treated(&self.symbol_table) {
                            Some(var_id)
                        } else {
                            None
                        }
                    } else if let Some(param_id) = self.local_scope.as_ref().unwrap().find_param(&ident.name) {
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

                let ty = self.generate_expression(*expression)?;

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
            Statement::Import { module_path, alias } => {
                // Parse the module path to extract module and symbol
                // For example: "test.test" -> module="test", symbol="test"
                // "std.collections.list" -> module="std.collections", symbol="list"
                let segments: Vec<&str> = module_path.split('.').collect();
                if segments.is_empty() {
                    return Err(SemanticError::ImportError {
                        module_path: module_path.clone(),
                        pos: SourcePos { line: 0, col: 0 },
                    });
                }

                // The last segment is the symbol name
                let symbol_name = segments.last().unwrap().to_string();
                
                // The module is everything except the last segment
                // If there's only one segment, we treat it as both module and symbol
                let module_part = if segments.len() > 1 {
                    segments[..segments.len() - 1].join(".")
                } else {
                    segments[0].to_string()
                };

                // Load the module from the module provider
                let source = self.module_provider.load_module(&module_part)
                    .ok_or_else(|| SemanticError::ImportError {
                        module_path: module_part.clone(),
                        pos: SourcePos {
                            line: 0,
                            col: 0,
                        },
                    })?;

                // Compile the imported module if not already compiled
                if !self.imported_modules.contains_key(&module_part) {
                    // Recursively compile the imported module
                    let imported_module = self.compile_module(source)?;
                    self.imported_modules.insert(module_part.clone(), imported_module);
                }

                // Get the binding name (use alias if provided, otherwise use symbol name)
                let binding_name = if let Some(ref alias_name) = alias {
                    alias_name.clone()
                } else {
                    symbol_name.clone()
                };

                // Find and import only the specified symbol from the module
                let imported_module = self.imported_modules.get(&module_part).unwrap();
                let mut found = false;
                for symbol in &imported_module.symbols {
                    // Only import the symbol that matches the requested name
                    if symbol.name == symbol_name {
                        // Add the symbol to our symbol table with the binding name (no prefix)
                        let new_symbol = Symbol::new(
                            binding_name.clone(),
                            symbol.qualified_name.clone(),
                            symbol.kind.clone(),
                        );
                        
                        // Add symbol to the flat list and make it a child of the current scope
                        let current_id = *self.symbol_table.symbol_chain.last().unwrap();
                        let symbol_id = self.symbol_table.symbols.len() as u32;
                        self.symbol_table.symbols.push(new_symbol);
                        
                        // Add as child of current scope so it can be found by find_symbol_in_scope
                        self.symbol_table.symbols[current_id as usize]
                            .children
                            .insert(binding_name.clone(), symbol_id);
                        
                        found = true;
                        break;
                    }
                }

                if !found {
                    return Err(SemanticError::Other(format!(
                        "Symbol '{}' not found in module '{}'",
                        symbol_name, module_part
                    )));
                }

                Ok(Type::Primitive(PrimitiveType::Unit))
            }
        }
    }
}
