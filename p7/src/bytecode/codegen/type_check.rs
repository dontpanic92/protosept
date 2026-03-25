use crate::ast::Type as ParsedType;
use crate::errors::{SemanticError, SourcePos};
use crate::semantic::{PrimitiveType, SymbolKind, Type, TypeDefinition, TypeId};

use super::{Generator, SaResult};

impl Generator {
    fn type_from_id(&self, type_id: TypeId) -> SaResult<Type> {
        match self.symbol_table.get_type_checked(type_id) {
            Some(TypeDefinition::Struct(_)) => Ok(Type::Struct(type_id)),
            Some(TypeDefinition::Enum(_)) => Ok(Type::Enum(type_id)),
            Some(TypeDefinition::Proto(_)) => Ok(Type::Proto(type_id)),
            None => Err(SemanticError::Other(format!(
                "Type id {} not found in symbol table",
                type_id
            ))),
        }
    }

    pub(super) fn resolve_qualified_type_name(
        &mut self,
        name: &str,
        line: usize,
        col: usize,
    ) -> SaResult<Type> {
        // Check if the full qualified name already exists in the symbol table
        // (handles synthetic names from monomorphization like "mod.types.Foo")
        if let Some(existing) = self.symbol_table.find_symbol_by_qualified_name(name) {
            if let SymbolKind::Type(type_id) = existing.kind {
                return self.type_from_id(type_id);
            }
        }

        let mut parts = name.split('.').collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(SemanticError::TypeNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            });
        }

        let module_alias = parts.remove(0).to_string();
        let member_name = parts.join(".");

        let symbol_id = self
            .symbol_table
            .find_symbol_in_scope(&module_alias)
            .ok_or_else(|| SemanticError::TypeNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            })?;

        let symbol =
            self.symbol_table
                .get_symbol(symbol_id)
                .ok_or_else(|| SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                })?;

        let module_id = match symbol.kind {
            SymbolKind::Module(module_id) => module_id,
            _ => {
                return Err(SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                });
            }
        };

        let module_path = self
            .symbol_table
            .get_module(module_id)
            .map(|m| m.path.clone())
            .ok_or_else(|| SemanticError::TypeNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            })?;

        let module = self
            .imported_modules
            .get(&module_path)
            .cloned()
            .ok_or_else(|| SemanticError::TypeNotFound {
                name: name.to_string(),
                pos: SourcePos::at(line, col),
            })?;

        let (imported_type_id, qualified_name) = {
            let root = module
                .symbols
                .get(0)
                .ok_or_else(|| SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                })?;

            let child_id =
                root.children
                    .get(&member_name)
                    .ok_or_else(|| SemanticError::TypeNotFound {
                        name: name.to_string(),
                        pos: SourcePos::at(line, col),
                    })?;

            let member = module.symbols.get(*child_id as usize).ok_or_else(|| {
                SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                }
            })?;

            match member.kind {
                SymbolKind::Type(type_id) => (type_id, member.qualified_name.clone()),
                _ => {
                    return Err(SemanticError::TypeNotFound {
                        name: name.to_string(),
                        pos: SourcePos::at(line, col),
                    });
                }
            }
        };

        if let Some(existing_symbol) = self
            .symbol_table
            .find_symbol_by_qualified_name(&qualified_name)
        {
            if let SymbolKind::Type(existing_type_id) = existing_symbol.kind {
                return self.type_from_id(existing_type_id);
            }
        }

        let mut type_map = std::collections::HashMap::new();
        let new_type_id = self.import_type_from_module(&module, imported_type_id, &mut type_map)?;

        let new_symbol = crate::semantic::Symbol::new(
            qualified_name.clone(),
            qualified_name.clone(),
            SymbolKind::Type(new_type_id),
        );
        self.symbol_table.insert_symbol(new_symbol);

        self.type_from_id(new_type_id)
    }

    pub(super) fn types_compatible(&self, actual: &Type, expected: &Type) -> bool {
        // Direct equality
        if actual == expected {
            return true;
        }

        // Handle references
        match (actual, expected) {
            (Type::Reference(a), Type::Reference(e)) => {
                return a == e;
            }
            (Type::MutableReference(a), Type::MutableReference(e)) => {
                return a == e;
            }
            (Type::MutableReference(a), Type::Reference(e)) => {
                return a == e;
            }
            _ => {}
        }

        // Allow implicit int -> float promotion (spec §15.1.2)
        // Note: float -> int requires explicit conversion, not allowed implicitly
        match (actual, expected) {
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Float)) => {
                return true;
            }
            _ => {}
        }

        false
    }

    pub(super) fn get_semantic_type(&mut self, parsed_type: &ParsedType) -> SaResult<Type> {
        match parsed_type {
            ParsedType::Identifier(identifier) => {
                if identifier.name.contains('.') {
                    return self.resolve_qualified_type_name(
                        &identifier.name,
                        identifier.line,
                        identifier.col,
                    );
                }

                if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
                    Ok(ty)
                } else {
                    Err(SemanticError::TypeNotFound {
                        name: identifier.name.clone(),
                        pos: identifier.pos(),
                    })
                }
            }
            ParsedType::Reference(r) => {
                let ty = self.get_semantic_type(r)?;
                Ok(Type::Reference(Box::new(ty)))
            }
            ParsedType::MutableReference(r) => {
                let ty = self.get_semantic_type(r)?;
                Ok(Type::MutableReference(Box::new(ty)))
            }
            ParsedType::Array(a) => {
                let ty = self.get_semantic_type(a)?;
                Ok(Type::Array(Box::new(ty)))
            }
            ParsedType::Nullable(n) => {
                let ty = self.get_semantic_type(n)?;
                Ok(Type::Nullable(Box::new(ty)))
            }
            ParsedType::Generic { base, type_args } => {
                // Handle box<T> specially (builtin generic type)
                if base.name == "box" {
                    if type_args.len() != 1 {
                        return Err(SemanticError::Other(format!(
                            "box<T> requires exactly one type argument, found {} at line {} column {}",
                            type_args.len(),
                            base.line,
                            base.col
                        )));
                    }
                    let inner_ty = self.get_semantic_type(&type_args[0])?;
                    return Ok(Type::BoxType(Box::new(inner_ty)));
                }

                // Handle array<T> specially (builtin generic type)
                if base.name == "array" {
                    if type_args.len() != 1 {
                        return Err(SemanticError::Other(format!(
                            "array<T> requires exactly one type argument, found {} at line {} column {}",
                            type_args.len(),
                            base.line,
                            base.col
                        )));
                    }
                    let inner_ty = self.get_semantic_type(&type_args[0])?;
                    return Ok(Type::Array(Box::new(inner_ty)));
                }

                // Handle HashMap<K, V> specially (builtin generic type)
                if base.name == "HashMap" {
                    if type_args.len() != 2 {
                        return Err(SemanticError::Other(format!(
                            "HashMap<K, V> requires exactly two type arguments, found {} at line {} column {}",
                            type_args.len(),
                            base.line,
                            base.col
                        )));
                    }
                    let key_ty = self.get_semantic_type(&type_args[0])?;
                    let val_ty = self.get_semantic_type(&type_args[1])?;
                    return Ok(Type::Map(Box::new(key_ty), Box::new(val_ty)));
                }

                // Implement proper generic type resolution with monomorphization

                // First, find the base generic type
                let base_type = if base.name.contains('.') {
                    self.resolve_qualified_type_name(&base.name, base.line, base.col)?
                } else {
                    self.require_type_from_identifier(base)?
                };

                // Resolve all type arguments
                let resolved_type_args = self.resolve_type_args(type_args)?;

                // For now, only handle struct and enum monomorphization
                match base_type {
                    Type::Struct(base_type_id) => self.monomorphize_struct(
                        base_type_id,
                        resolved_type_args,
                        &base.name,
                        base.line,
                        base.col,
                    ),
                    Type::Enum(base_type_id) => self.monomorphize_enum(
                        base_type_id,
                        resolved_type_args,
                        &base.name,
                        base.line,
                        base.col,
                    ),
                    _ => {
                        // For non-struct/enum types, just return the base type for now
                        // TODO: implement monomorphization for other types if needed
                        Ok(base_type)
                    }
                }
            }
            ParsedType::Function {
                param_types,
                return_type,
            } => {
                let params = param_types
                    .iter()
                    .map(|pt| self.get_semantic_type(pt))
                    .collect::<SaResult<Vec<Type>>>()?;
                let ret = self.get_semantic_type(return_type)?;
                Ok(Type::Function {
                    params,
                    return_type: Box::new(ret),
                })
            }
            ParsedType::Tuple(elements) => {
                let resolved = elements
                    .iter()
                    .map(|t| self.get_semantic_type(t))
                    .collect::<SaResult<Vec<Type>>>()?;
                Ok(Type::Tuple(resolved))
            }
        }
    }

    /// Check that a struct conforms to all declared protocols
    pub(super) fn check_struct_conformance(
        &self,
        type_id: TypeId,
        conforming_to: &[TypeId],
        line: usize,
        col: usize,
    ) -> SaResult<()> {
        // Get the type definition (struct or enum)
        let (type_name, qualified_name) = match &self.symbol_table.types[type_id as usize] {
            TypeDefinition::Struct(s) => ("Struct", s.qualified_name.clone()),
            TypeDefinition::Enum(e) => ("Enum", e.qualified_name.clone()),
            TypeDefinition::Proto(_) => {
                return Err(SemanticError::Other(
                    "Expected struct or enum type".to_string(),
                ));
            }
        };

        // For each protocol the type claims to conform to
        for &proto_id in conforming_to {
            let proto = match &self.symbol_table.types[proto_id as usize] {
                TypeDefinition::Proto(p) => p,
                _ => return Err(SemanticError::Other("Expected proto type".to_string())),
            };

            // Check each required method in the protocol
            for (method_name, param_types, return_type) in &proto.methods {
                // Find the corresponding method in the type
                let type_method = self.find_type_method(type_id, method_name);

                if type_method.is_none() {
                    return Err(SemanticError::Other(format!(
                        "{} '{}' does not implement required method '{}' from protocol '{}' at line {} column {}",
                        type_name, qualified_name, method_name, proto.qualified_name, line, col
                    )));
                }

                let (method_params, method_return_type) = type_method.unwrap();

                // Check parameter count matches
                if method_params.len() != param_types.len() {
                    return Err(SemanticError::Other(format!(
                        "Method '{}' in {} '{}' has {} parameters, but protocol '{}' requires {} parameters at line {} column {}",
                        method_name,
                        type_name,
                        qualified_name,
                        method_params.len(),
                        proto.qualified_name,
                        param_types.len(),
                        line,
                        col
                    )));
                }

                // Check each parameter type matches
                // Special handling for first parameter (self) which should be ref to the type/proto type
                for (i, (expected_type, actual_type)) in
                    param_types.iter().zip(method_params.iter()).enumerate()
                {
                    // For the first parameter (self), check if both are reference types
                    // The proto has `self: ref<Proto>` and the type has `self: ref<Type>`
                    // These should be considered compatible for conformance checking
                    if i == 0 {
                        let proto_is_ref_to_proto = matches!(expected_type, Type::Reference(inner) if matches!(**inner, Type::Proto(pid) if pid == proto_id));
                        let type_is_ref_to_self = matches!(actual_type, Type::Reference(inner) if matches!(**inner, Type::Struct(sid) if sid == type_id) || matches!(**inner, Type::Enum(eid) if eid == type_id));

                        if proto_is_ref_to_proto && type_is_ref_to_self {
                            // Both are reference types to their respective types, this is correct
                            continue;
                        }
                    }

                    if !self.types_equal(expected_type, actual_type) {
                        return Err(SemanticError::Other(format!(
                            "Method '{}' in {} '{}' has parameter {} with type '{}', but protocol '{}' requires type '{}' at line {} column {}",
                            method_name,
                            type_name,
                            qualified_name,
                            i,
                            self.type_to_string(actual_type),
                            proto.qualified_name,
                            self.type_to_string(expected_type),
                            line,
                            col
                        )));
                    }
                }

                // Check return type matches
                match (return_type, method_return_type) {
                    (Some(expected), Some(actual)) => {
                        // Both unit and no return type should be considered equivalent
                        let expected_is_unit =
                            matches!(expected, Type::Primitive(PrimitiveType::Unit));
                        let actual_is_unit = matches!(actual, Type::Primitive(PrimitiveType::Unit));

                        if expected_is_unit && actual_is_unit {
                            // Both return unit, this is fine
                        } else if !self.types_equal(&expected, &actual) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                self.type_to_string(&actual),
                                proto.qualified_name,
                                self.type_to_string(&expected),
                                line,
                                col
                            )));
                        }
                    }
                    (Some(expected), None) => {
                        // Proto requires a return type, but type method returns nothing (unit)
                        // If proto expects unit, this is fine
                        if !matches!(expected, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns nothing, but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                proto.qualified_name,
                                self.type_to_string(&expected),
                                line,
                                col
                            )));
                        }
                    }
                    (None, Some(actual)) => {
                        // Proto expects no return, but type returns something
                        // If type returns unit, this is fine
                        if !matches!(actual, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' expects no return type at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                self.type_to_string(&actual),
                                proto.qualified_name,
                                line,
                                col
                            )));
                        }
                    }
                    (None, None) => {
                        // Both return nothing, this is fine
                    }
                }
            }
        }

        Ok(())
    }

    /// Find a method in a type (struct or enum) by name and return its signature
    pub(super) fn find_type_method(
        &self,
        type_id: TypeId,
        method_name: &str,
    ) -> Option<(Vec<Type>, Option<Type>)> {
        // Search for a function with the qualified name type_name.method_name
        let qualified_name = match &self.symbol_table.types[type_id as usize] {
            TypeDefinition::Struct(s) => s.qualified_name.clone(),
            TypeDefinition::Enum(e) => e.qualified_name.clone(),
            TypeDefinition::Proto(_) => return None,
        };

        let qualified_method_name = format!("{}.{}", qualified_name, method_name);

        // Look through all symbols to find the method
        for symbol in &self.symbol_table.symbols {
            if symbol.qualified_name == qualified_method_name {
                if let SymbolKind::Function { func_id, .. } = symbol.kind {
                    // Get the function from the functions table
                    let func = self.symbol_table.get_function(func_id);
                    // Proto methods have return_type as Option<Type>, but Function has return_type as Type
                    // Convert Type::Primitive(Unit) to None for consistency
                    let return_type = if func.return_type == Type::Primitive(PrimitiveType::Unit) {
                        None
                    } else {
                        Some(func.return_type.clone())
                    };
                    return Some((func.params.clone(), return_type));
                }
            }
        }

        None
    }

    /// Helper to check if two types are equal
    pub(super) fn types_equal(&self, a: &Type, b: &Type) -> bool {
        match (a, b) {
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => self.types_equal(a, b),
            (Type::Reference(a), Type::Reference(b)) => self.types_equal(a, b),
            (Type::MutableReference(a), Type::MutableReference(b)) => self.types_equal(a, b),
            (Type::Struct(a), Type::Struct(b)) => a == b,
            (Type::Enum(a), Type::Enum(b)) => a == b,
            (Type::Proto(a), Type::Proto(b)) => a == b,
            (Type::BoxType(a), Type::BoxType(b)) => self.types_equal(a, b),
            (Type::Nullable(a), Type::Nullable(b)) => self.types_equal(a, b),
            (Type::Tuple(a), Type::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| self.types_equal(x, y))
            }
            (Type::Map(ka, va), Type::Map(kb, vb)) => {
                self.types_equal(ka, kb) && self.types_equal(va, vb)
            }
            _ => false,
        }
    }

    /// Helper to convert a type to a string for error messages
    pub(super) fn type_to_string(&self, ty: &Type) -> String {
        match ty {
            Type::Primitive(p) => format!("{:?}", p).to_lowercase(),
            Type::Array(inner) => format!("array<{}>", self.type_to_string(inner)),
            Type::Reference(inner) => format!("ref {}", self.type_to_string(inner)),
            Type::MutableReference(inner) => format!("ref mut {}", self.type_to_string(inner)),
            Type::Struct(id) => {
                // Check bounds to handle type IDs from imported modules
                if let Some(TypeDefinition::Struct(s)) = self.symbol_table.types.get(*id as usize) {
                    s.qualified_name.clone()
                } else {
                    format!("struct#{}", id)
                }
            }
            Type::Enum(id) => {
                // Check bounds to handle type IDs from imported modules
                if let Some(TypeDefinition::Enum(e)) = self.symbol_table.types.get(*id as usize) {
                    e.qualified_name.clone()
                } else {
                    format!("enum#{}", id)
                }
            }
            Type::Proto(id) => {
                // Check bounds to handle type IDs from imported modules
                if let Some(TypeDefinition::Proto(p)) = self.symbol_table.types.get(*id as usize) {
                    p.qualified_name.clone()
                } else {
                    format!("proto#{}", id)
                }
            }
            Type::BoxType(inner) => format!("box<{}>", self.type_to_string(inner)),
            Type::Nullable(inner) => format!("?{}", self.type_to_string(inner)),
            Type::Function { params, return_type } => {
                let param_strs: Vec<String> = params.iter().map(|p| self.type_to_string(p)).collect();
                format!("fn({}) -> {}", param_strs.join(", "), self.type_to_string(return_type))
            }
            Type::Tuple(elements) => {
                let elem_strs: Vec<String> = elements.iter().map(|t| self.type_to_string(t)).collect();
                format!("({})", elem_strs.join(", "))
            }
            Type::Map(k, v) => {
                format!("HashMap<{}, {}>", self.type_to_string(k), self.type_to_string(v))
            }
        }
    }
}
