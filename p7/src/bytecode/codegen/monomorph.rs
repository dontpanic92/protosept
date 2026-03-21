use crate::errors::SemanticError;
use crate::errors::SourcePos;
use crate::{
    ast::{Identifier, Type as ParsedType},
    semantic::{
        Enum, Function, FunctionId, PrimitiveType, Struct, Symbol, SymbolKind, Type,
        TypeDefinition, TypeId,
    },
};

use super::{Generator, SYNTHETIC_COL, SYNTHETIC_LINE, SaResult};

impl Generator {
    /// Monomorphize a generic struct with concrete type arguments
    pub(super) fn monomorphize_struct(
        &mut self,
        base_type_id: TypeId,
        type_args: Vec<Type>,
        _base_name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        // Check cache first
        let cache_key = (base_type_id, type_args.clone());
        if let Some(&cached_type_id) = self.symbol_table.monomorphization_cache.get(&cache_key) {
            return Ok(Type::Struct(cached_type_id));
        }

        // Get the base generic struct definition
        let base_struct = match self.symbol_table.get_type(base_type_id) {
            TypeDefinition::Struct(s) => s.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "struct".to_string(),
                    rhs: "non-struct".to_string(),
                    pos: SourcePos::at(line, col),
                });
            }
        };

        // Validate number of type arguments matches type parameters
        Self::validate_type_arg_count(
            base_struct.type_parameters.len(),
            type_args.len(),
            line,
            col,
        )?;

        // If the struct has no type parameters, just return it as-is
        if base_struct.type_parameters.is_empty() {
            return Ok(Type::Struct(base_type_id));
        }

        // Get the parsed field types
        let parsed_field_types = base_struct.generic_field_types.as_ref().ok_or_else(|| {
            SemanticError::TypeMismatch {
                lhs: "generic struct".to_string(),
                rhs: "missing generic field types".to_string(),
                pos: SourcePos::at(line, col),
            }
        })?;

        // Build type parameter substitution map for parsed types
        let mut parsed_type_substitution: std::collections::HashMap<String, ParsedType> =
            std::collections::HashMap::new();
        for (param_name, concrete_type) in base_struct.type_parameters.iter().zip(type_args.iter())
        {
            // Convert the semantic Type back to ParsedType for substitution
            let parsed_type = self.type_to_parsed_type(concrete_type);
            parsed_type_substitution.insert(param_name.clone(), parsed_type);
        }

        // Substitute and resolve field types
        let mut monomorphized_fields: Vec<(String, Type)> = Vec::new();
        for (i, (field_name, _)) in base_struct.fields.iter().enumerate() {
            let parsed_field_type = &parsed_field_types[i];
            let substituted_parsed_type =
                self.substitute_parsed_type(parsed_field_type, &parsed_type_substitution);
            let resolved_type = self.get_semantic_type(&substituted_parsed_type)?;
            monomorphized_fields.push((field_name.clone(), resolved_type));
        }

        // Create monomorphized struct name
        let type_args_str = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let monomorphized_name = format!("{}<{}>", base_struct.qualified_name, type_args_str);

        // Create the monomorphized struct
        let monomorphized_struct = Struct {
            qualified_name: monomorphized_name,
            fields: monomorphized_fields,
            field_defaults: base_struct.field_defaults.clone(),
            attributes: base_struct.attributes.clone(),
            type_parameters: Vec::new(), // Monomorphized structs have no type parameters
            generic_field_types: None,
            monomorphization: Some((base_type_id, type_args.clone())),
            conforming_to: base_struct.conforming_to.clone(),
            methods: base_struct.methods.clone(), // Copy methods from base
            source_module: base_struct.source_module.clone(),
        };

        // Add to type table
        let new_type_id = self
            .symbol_table
            .add_type(TypeDefinition::Struct(monomorphized_struct));

        // Cache it
        self.symbol_table
            .monomorphization_cache
            .insert(cache_key, new_type_id);

        Ok(Type::Struct(new_type_id))
    }

    pub(super) fn monomorphize_enum(
        &mut self,
        base_type_id: TypeId,
        type_args: Vec<Type>,
        _base_name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        // Check cache first
        let cache_key = (base_type_id, type_args.clone());
        if let Some(&cached_type_id) = self.symbol_table.monomorphization_cache.get(&cache_key) {
            return Ok(Type::Enum(cached_type_id));
        }

        // Get the base generic enum definition
        let base_enum = match self.symbol_table.get_type(base_type_id) {
            TypeDefinition::Enum(e) => e.clone(),
            _ => {
                return Err(SemanticError::TypeMismatch {
                    lhs: "enum".to_string(),
                    rhs: "non-enum".to_string(),
                    pos: SourcePos::at(line, col),
                });
            }
        };

        // Validate number of type arguments matches type parameters
        Self::validate_type_arg_count(base_enum.type_parameters.len(), type_args.len(), line, col)?;

        // If the enum has no type parameters, just return it as-is
        if base_enum.type_parameters.is_empty() {
            return Ok(Type::Enum(base_type_id));
        }

        // Get the parsed variant field types
        let parsed_variant_types = base_enum.generic_variant_types.as_ref().ok_or_else(|| {
            SemanticError::TypeMismatch {
                lhs: "generic enum".to_string(),
                rhs: "missing generic variant types".to_string(),
                pos: SourcePos::at(line, col),
            }
        })?;

        // Build type parameter substitution map for parsed types
        let mut parsed_type_substitution: std::collections::HashMap<String, ParsedType> =
            std::collections::HashMap::new();
        for (param_name, concrete_type) in base_enum.type_parameters.iter().zip(type_args.iter()) {
            // Convert the semantic Type back to ParsedType for substitution
            let parsed_type = self.type_to_parsed_type(concrete_type);
            parsed_type_substitution.insert(param_name.clone(), parsed_type);
        }

        // Substitute and resolve variant field types
        let mut monomorphized_variants: Vec<(String, Vec<Type>)> = Vec::new();
        for (i, (variant_name, _)) in base_enum.variants.iter().enumerate() {
            let parsed_field_types = &parsed_variant_types[i];
            let mut resolved_field_types = Vec::new();

            for parsed_field_type in parsed_field_types {
                let substituted_parsed_type =
                    self.substitute_parsed_type(parsed_field_type, &parsed_type_substitution);
                let resolved_type = self.get_semantic_type(&substituted_parsed_type)?;
                resolved_field_types.push(resolved_type);
            }

            monomorphized_variants.push((variant_name.clone(), resolved_field_types));
        }

        // Create monomorphized enum name
        let type_args_str = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let monomorphized_name = format!("{}<{}>", base_enum.qualified_name, type_args_str);

        // Create the monomorphized enum
        let monomorphized_enum = Enum {
            qualified_name: monomorphized_name,
            variants: monomorphized_variants,
            attributes: base_enum.attributes.clone(),
            type_parameters: Vec::new(), // Monomorphized enums have no type parameters
            generic_variant_types: None,
            monomorphization: Some((base_type_id, type_args.clone())),
            conforming_to: base_enum.conforming_to.clone(),
            methods: base_enum.methods.clone(), // Copy methods from base
            source_module: base_enum.source_module.clone(),
        };

        // Add to type table
        let new_type_id = self
            .symbol_table
            .add_type(TypeDefinition::Enum(monomorphized_enum));

        // Cache it
        self.symbol_table
            .monomorphization_cache
            .insert(cache_key, new_type_id);

        Ok(Type::Enum(new_type_id))
    }

    /// Monomorphize a generic function with concrete type arguments
    pub(super) fn monomorphize_function(
        &mut self,
        base_func_id: FunctionId,
        type_args: Vec<Type>,
        base_name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<(u32, FunctionId, u32)> {
        // Returns (address, func_id, symbol_id)
        // Check cache first
        let cache_key = (base_func_id, type_args.clone());
        if let Some(&cached_func_id) = self
            .symbol_table
            .function_monomorphization_cache
            .get(&cache_key)
        {
            // Find the address of the cached function
            let cached_func = self.symbol_table.get_function(cached_func_id);

            // Find the symbol with this function's qualified name
            for (idx, symbol) in self.symbol_table.symbols.iter().enumerate() {
                if symbol.qualified_name == cached_func.qualified_name {
                    if let SymbolKind::Function { address, func_id } = symbol.kind {
                        return Ok((address, func_id, idx as u32));
                    }
                }
            }

            return Err(SemanticError::TypeMismatch {
                lhs: "function symbol".to_string(),
                rhs: "not found".to_string(),
                pos: SourcePos::at(line, col),
            });
        }

        // Get the base generic function definition
        let base_func = self.symbol_table.get_function(base_func_id).clone();

        // Validate number of type arguments matches type parameters
        Self::validate_type_arg_count(base_func.type_parameters.len(), type_args.len(), line, col)?;

        // If the function has no type parameters, just return it as-is
        if base_func.type_parameters.is_empty() {
            // Find the address and symbol_id of the base function
            for (idx, symbol) in self.symbol_table.symbols.iter().enumerate() {
                if symbol.qualified_name == base_func.qualified_name {
                    if let SymbolKind::Function { address, func_id } = symbol.kind {
                        return Ok((address, func_id, idx as u32));
                    }
                }
            }
            return Err(SemanticError::TypeMismatch {
                lhs: "function symbol".to_string(),
                rhs: "not found".to_string(),
                pos: SourcePos::at(line, col),
            });
        }

        // Get the parsed parameter types and return type
        let parsed_param_types =
            base_func
                .generic_param_types
                .as_ref()
                .ok_or_else(|| SemanticError::TypeMismatch {
                    lhs: "generic function".to_string(),
                    rhs: "missing generic parameter types".to_string(),
                    pos: SourcePos::at(line, col),
                })?;

        let parsed_return_type = base_func.generic_return_type.as_ref();

        let body = base_func
            .generic_body
            .as_ref()
            .ok_or_else(|| SemanticError::TypeMismatch {
                lhs: "generic function".to_string(),
                rhs: "missing generic body".to_string(),
                pos: SourcePos::at(line, col),
            })?;

        // Build type parameter substitution map for parsed types
        let mut parsed_type_substitution: std::collections::HashMap<String, ParsedType> =
            std::collections::HashMap::new();
        for (param_name, concrete_type) in base_func.type_parameters.iter().zip(type_args.iter()) {
            // Convert the semantic Type back to ParsedType for substitution
            let parsed_type = self.type_to_parsed_type(concrete_type);
            parsed_type_substitution.insert(param_name.clone(), parsed_type);
        }

        // Substitute and resolve parameter types
        let mut monomorphized_params: Vec<Type> = Vec::new();
        for parsed_param_type in parsed_param_types.iter() {
            let substituted_parsed_type =
                self.substitute_parsed_type(parsed_param_type, &parsed_type_substitution);
            let resolved_type = self.get_semantic_type(&substituted_parsed_type)?;
            monomorphized_params.push(resolved_type);
        }

        // Substitute and resolve return type
        let monomorphized_return_type = if let Some(parsed_ret) = parsed_return_type {
            let substituted_parsed_type =
                self.substitute_parsed_type(parsed_ret, &parsed_type_substitution);
            self.get_semantic_type(&substituted_parsed_type)?
        } else {
            Type::Primitive(PrimitiveType::Unit)
        };

        // Create monomorphized function name
        let type_args_str = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let monomorphized_name = format!("{}<{}>", base_func.qualified_name, type_args_str);

        // Create the monomorphized function metadata
        let monomorphized_func = Function {
            qualified_name: monomorphized_name.clone(),
            params: monomorphized_params.clone(),
            param_names: base_func.param_names.clone(),
            param_defaults: base_func.param_defaults.clone(),
            return_type: monomorphized_return_type,
            attributes: base_func.attributes.clone(),
            intrinsic_name: base_func.intrinsic_name.clone(),
            type_parameters: Vec::new(), // Monomorphized functions have no type parameters
            generic_param_types: None,
            generic_return_type: None,
            generic_body: None,
            monomorphization: Some((base_func_id, type_args.clone())),
        };

        // Add to function table
        let new_func_id = self.symbol_table.add_function(monomorphized_func.clone());

        // Cache it
        self.symbol_table
            .function_monomorphization_cache
            .insert(cache_key, new_func_id);

        // Create symbol for the monomorphized function with placeholder address
        // We'll generate the actual bytecode later to avoid inline generation
        let symbol = Symbol::new(
            base_name.to_string(),
            monomorphized_name.clone(),
            SymbolKind::Function {
                func_id: new_func_id,
                address: 0xFFFFFFFF, // Placeholder - will be updated when bytecode is generated
            },
        );

        self.symbol_table.push_symbol(symbol);
        let symbol_id = (self.symbol_table.symbols.len() - 1) as u32;

        // Queue this monomorphization for later bytecode generation
        self.pending_monomorphizations.push((
            symbol_id,
            new_func_id,
            body.clone(),
            monomorphized_func.param_names.clone(),
            monomorphized_params.clone(),
        ));

        // Don't generate bytecode here - it will be generated later
        // Just return the symbol_id so the caller can emit a call instruction
        self.symbol_table.pop_symbol();

        Ok((0xFFFFFFFF, new_func_id, symbol_id))
    }

    /// Convert a semantic Type to a ParsedType (for substitution purposes)
    pub(super) fn type_to_parsed_type(&self, ty: &Type) -> ParsedType {
        match ty {
            Type::Primitive(p) => {
                let name = match p {
                    PrimitiveType::Int => "int",
                    PrimitiveType::Float => "float",
                    PrimitiveType::Bool => "bool",
                    PrimitiveType::Char => "char",
                    PrimitiveType::String => "string",
                    PrimitiveType::Unit => "unit",
                };
                ParsedType::Identifier(Identifier {
                    name: name.to_string(),
                    line: SYNTHETIC_LINE,
                    col: SYNTHETIC_COL,
                })
            }
            Type::Reference(inner) => {
                ParsedType::Reference(Box::new(self.type_to_parsed_type(inner)))
            }
            Type::Array(inner) => ParsedType::Array(Box::new(self.type_to_parsed_type(inner))),
            Type::BoxType(inner) => ParsedType::Generic {
                base: Identifier {
                    name: "box".to_string(),
                    line: SYNTHETIC_LINE,
                    col: SYNTHETIC_COL,
                },
                type_args: vec![self.type_to_parsed_type(inner)],
            },
            Type::Struct(type_id) => {
                // Get the struct name from the symbol table
                let type_def = self.symbol_table.get_type(*type_id);
                if let TypeDefinition::Struct(s) = type_def {
                    ParsedType::Identifier(Identifier {
                        name: s.qualified_name.clone(),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                } else {
                    // Fallback
                    ParsedType::Identifier(Identifier {
                        name: format!("struct_{}", type_id),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                }
            }
            Type::Enum(type_id) => {
                // Get the enum name from the symbol table
                let type_def = self.symbol_table.get_type(*type_id);
                if let TypeDefinition::Enum(e) = type_def {
                    ParsedType::Identifier(Identifier {
                        name: e.qualified_name.clone(),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                } else {
                    // Fallback
                    ParsedType::Identifier(Identifier {
                        name: format!("enum_{}", type_id),
                        line: SYNTHETIC_LINE,
                        col: SYNTHETIC_COL,
                    })
                }
            }
            _ => {
                // For other types, create a simple identifier
                ParsedType::Identifier(Identifier {
                    name: ty.to_string(),
                    line: SYNTHETIC_LINE,
                    col: SYNTHETIC_COL,
                })
            }
        }
    }

    /// Substitute type parameters in a ParsedType
    pub(super) fn substitute_parsed_type(
        &self,
        parsed_type: &ParsedType,
        substitution: &std::collections::HashMap<String, ParsedType>,
    ) -> ParsedType {
        match parsed_type {
            ParsedType::Identifier(ident) => {
                // Check if this identifier is a type parameter
                if let Some(concrete_type) = substitution.get(&ident.name) {
                    concrete_type.clone()
                } else {
                    parsed_type.clone()
                }
            }
            ParsedType::Reference(inner) => {
                ParsedType::Reference(Box::new(self.substitute_parsed_type(inner, substitution)))
            }
            ParsedType::Array(inner) => {
                ParsedType::Array(Box::new(self.substitute_parsed_type(inner, substitution)))
            }
            ParsedType::Nullable(inner) => {
                ParsedType::Nullable(Box::new(self.substitute_parsed_type(inner, substitution)))
            }
            ParsedType::Generic { base, type_args } => {
                let substituted_args: Vec<ParsedType> = type_args
                    .iter()
                    .map(|arg| self.substitute_parsed_type(arg, substitution))
                    .collect();
                ParsedType::Generic {
                    base: base.clone(),
                    type_args: substituted_args,
                }
            }
            ParsedType::Function {
                param_types,
                return_type,
            } => {
                let substituted_params: Vec<ParsedType> = param_types
                    .iter()
                    .map(|p| self.substitute_parsed_type(p, substitution))
                    .collect();
                let substituted_ret =
                    self.substitute_parsed_type(return_type, substitution);
                ParsedType::Function {
                    param_types: substituted_params,
                    return_type: Box::new(substituted_ret),
                }
            }
            ParsedType::Tuple(elements) => {
                let substituted_elements: Vec<ParsedType> = elements
                    .iter()
                    .map(|t| self.substitute_parsed_type(t, substitution))
                    .collect();
                ParsedType::Tuple(substituted_elements)
            }
        }
    }
}
