use crate::ast::Type as ParsedType;
use crate::errors::{SemanticError, SourcePos};
use crate::intern::InternedString;
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
        if let Some(existing) = self.symbol_table.find_symbol_by_qualified_name(name)
            && let SymbolKind::Type(type_id) = existing.kind
        {
            return self.type_from_id(type_id);
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
                .first()
                .ok_or_else(|| SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                })?;

            let child_id = root.children.get(member_name.as_str()).ok_or_else(|| {
                SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                }
            })?;

            let member = module.symbols.get(*child_id as usize).ok_or_else(|| {
                SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                }
            })?;

            if !self.imported_symbol_is_public(&module, member) {
                return Err(SemanticError::TypeNotFound {
                    name: name.to_string(),
                    pos: SourcePos::at(line, col),
                });
            }

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
            && let SymbolKind::Type(existing_type_id) = existing_symbol.kind
        {
            return self.type_from_id(existing_type_id);
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
                if a == e {
                    return true;
                }
                // `ref<T> -> ref<P>` when T conforms to P (§18.5, §18.6).
                if self.inner_type_conforms_to_proto(a, e) {
                    return true;
                }
                return false;
            }
            (Type::MutableReference(a), Type::MutableReference(e)) => {
                return a == e;
            }
            (Type::MutableReference(a), Type::Reference(e)) => {
                return a == e;
            }
            (Type::BoxType(a), Type::BoxType(e)) => {
                if a == e {
                    return true;
                }
                // `box<T> -> box<P>` when T conforms to P (§18.5, §18.6).
                if self.inner_type_conforms_to_proto(a, e) {
                    return true;
                }
                return false;
            }
            _ => {}
        }

        // Allow implicit int -> float promotion (spec §15.1.2)
        // Note: float -> int requires explicit conversion, not allowed implicitly
        if let (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::Float)) =
            (actual, expected)
        {
            return true;
        }

        // Allow implicit `T -> ?T` widening at checking/expected-type sites (spec §3.5/§15.2).
        // Non-nullable values may flow into a nullable expected type when the inner type
        // matches. Codegen for the widening lives in
        // generate_expression_with_expected_type, which emits `WrapNullable`.
        if let Type::Nullable(inner) = expected
            && !matches!(actual, Type::Nullable(_))
            && self.types_compatible(actual, inner)
        {
            return true;
        }

        false
    }

    /// Return true when `actual_inner` is a struct or enum that declares
    /// conformance to the proto in `expected_inner`. Used to recognize
    /// `box<T> -> box<P>` and `ref<T> -> ref<P>` as compatible after
    /// `generate_expression_with_expected_type` emits the coercion.
    fn inner_type_conforms_to_proto(&self, actual_inner: &Type, expected_inner: &Type) -> bool {
        let Type::Proto(proto_id) = expected_inner else {
            return false;
        };
        let type_id = match actual_inner {
            Type::Struct(sid) => *sid,
            Type::Enum(eid) => *eid,
            _ => return false,
        };
        match self.symbol_table.types.get(type_id as usize) {
            Some(TypeDefinition::Struct(s)) => s.conforming_to.contains(proto_id),
            Some(TypeDefinition::Enum(e)) => e.conforming_to.contains(proto_id),
            _ => false,
        }
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
                        name: identifier.name.to_string(),
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
                    Type::Proto(base_type_id) => {
                        let expected = match self.symbol_table.types.get(base_type_id as usize) {
                            Some(TypeDefinition::Proto(p)) => p.type_parameters.len(),
                            _ => 0,
                        };
                        if expected != resolved_type_args.len() {
                            return Err(SemanticError::Other(format!(
                                "proto '{}' expects {} type argument(s), found {} at line {} column {}",
                                base.name,
                                expected,
                                resolved_type_args.len(),
                                base.line,
                                base.col
                            )));
                        }
                        Ok(Type::ProtoGeneric {
                            base: base_type_id,
                            args: resolved_type_args,
                        })
                    }
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

    /// Check that a struct/enum conforms to every declared protocol entry.
    ///
    /// `conforming_to` and `conforming_to_args` are parallel — entry `i`
    /// represents `[proto_id, args]`. For generic protos, the proto's
    /// `method_templates` (in parsed-AST form) are walked, each `T`/`Iter`
    /// type-parameter occurrence is substituted with the matching arg, then
    /// `get_semantic_type` converts the substituted shape to a checking
    /// signature. For non-generic protos the args slice is empty and we
    /// fall through to the existing eagerly-resolved `proto.methods`.
    pub(super) fn check_struct_conformance(
        &mut self,
        type_id: TypeId,
        conforming_to: &[TypeId],
        conforming_to_args: &[Vec<Type>],
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

        for (entry_idx, &proto_id) in conforming_to.iter().enumerate() {
            let empty_args: Vec<Type> = Vec::new();
            let proto_args = conforming_to_args.get(entry_idx).unwrap_or(&empty_args);

            // Build the specialized method-signature list for this proto.
            // For generic protos we substitute `method_templates`; for
            // non-generic protos `methods` is the source of truth.
            let (proto_qualified_name, expected_methods): (
                InternedString,
                Vec<(InternedString, Vec<Type>, Option<Type>)>,
            ) = {
                let proto = match &self.symbol_table.types[proto_id as usize] {
                    TypeDefinition::Proto(p) => p,
                    _ => return Err(SemanticError::Other("Expected proto type".to_string())),
                };

                if proto.type_parameters.is_empty() {
                    (proto.qualified_name.clone(), proto.methods.clone())
                } else {
                    let templates = proto.method_templates.clone();
                    let type_params = proto.type_parameters.clone();
                    let qname = proto.qualified_name.clone();

                    // Build substitution from type param name → ParsedType.
                    // Also map `Self` to the proto's qualified name so that
                    // receiver shortcuts like `box self` (parsed as `box<Self>`)
                    // resolve back to the base proto when the template is
                    // re-checked at each conformance site.
                    let proto_qualified_for_self = proto.qualified_name.clone();
                    let mut substitution: std::collections::HashMap<
                        InternedString,
                        crate::ast::Type,
                    > = type_params
                        .iter()
                        .zip(proto_args.iter())
                        .map(|(name, arg)| (name.clone(), self.type_to_parsed_type(arg)))
                        .collect();
                    substitution.insert(
                        InternedString::from("Self"),
                        crate::ast::Type::Identifier(crate::ast::Identifier {
                            name: proto_qualified_for_self,
                            line: 0,
                            col: 0,
                        }),
                    );

                    let mut resolved = Vec::with_capacity(templates.len());
                    for (mname, params, return_ty) in templates {
                        let mut resolved_params = Vec::with_capacity(params.len());
                        for p in &params {
                            let substituted = self.substitute_parsed_type(p, &substitution);
                            resolved_params.push(self.get_semantic_type(&substituted)?);
                        }
                        let resolved_ret = match return_ty {
                            Some(ref r) => {
                                let substituted = self.substitute_parsed_type(r, &substitution);
                                Some(self.get_semantic_type(&substituted)?)
                            }
                            None => None,
                        };
                        resolved.push((mname, resolved_params, resolved_ret));
                    }
                    (qname, resolved)
                }
            };

            for (method_name, param_types, return_type) in &expected_methods {
                let type_method = self.find_type_method(type_id, method_name);

                if type_method.is_none() {
                    return Err(SemanticError::Other(format!(
                        "{} '{}' does not implement required method '{}' from protocol '{}' at line {} column {}",
                        type_name, qualified_name, method_name, proto_qualified_name, line, col
                    )));
                }

                let (method_params, method_return_type) = type_method.unwrap();

                if method_params.len() != param_types.len() {
                    return Err(SemanticError::Other(format!(
                        "Method '{}' in {} '{}' has {} parameters, but protocol '{}' requires {} parameters at line {} column {}",
                        method_name,
                        type_name,
                        qualified_name,
                        method_params.len(),
                        proto_qualified_name,
                        param_types.len(),
                        line,
                        col
                    )));
                }

                for (i, (expected_type, actual_type)) in
                    param_types.iter().zip(method_params.iter()).enumerate()
                {
                    if i == 0 {
                        let inner_is_self = |inner: &Type| -> bool {
                            matches!(inner, Type::Struct(sid) if *sid == type_id)
                                || matches!(inner, Type::Enum(eid) if *eid == type_id)
                        };
                        let inner_is_proto = |inner: &Type| -> bool {
                            matches!(inner, Type::Proto(pid) if *pid == proto_id)
                                || matches!(
                                    inner,
                                    Type::ProtoGeneric { base, .. } if *base == proto_id
                                )
                        };
                        let receiver_forms_match = match (expected_type, actual_type) {
                            (Type::Reference(e), Type::Reference(a)) => {
                                inner_is_proto(e) && inner_is_self(a)
                            }
                            (Type::MutableReference(e), Type::MutableReference(a)) => {
                                inner_is_proto(e) && inner_is_self(a)
                            }
                            (Type::BoxType(e), Type::BoxType(a)) => {
                                inner_is_proto(e) && inner_is_self(a)
                            }
                            _ => false,
                        };
                        if receiver_forms_match {
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
                            proto_qualified_name,
                            self.type_to_string(expected_type),
                            line,
                            col
                        )));
                    }
                }

                match (return_type, method_return_type) {
                    (Some(expected), Some(actual)) => {
                        let expected_is_unit =
                            matches!(expected, Type::Primitive(PrimitiveType::Unit));
                        let actual_is_unit = matches!(actual, Type::Primitive(PrimitiveType::Unit));

                        if expected_is_unit && actual_is_unit {
                        } else if !self.types_equal(expected, &actual) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                self.type_to_string(&actual),
                                proto_qualified_name,
                                self.type_to_string(expected),
                                line,
                                col
                            )));
                        }
                    }
                    (Some(expected), None) => {
                        if !matches!(expected, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns nothing, but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                proto_qualified_name,
                                self.type_to_string(expected),
                                line,
                                col
                            )));
                        }
                    }
                    (None, Some(actual)) => {
                        if !matches!(actual, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' expects no return type at line {} column {}",
                                method_name,
                                type_name,
                                qualified_name,
                                self.type_to_string(&actual),
                                proto_qualified_name,
                                line,
                                col
                            )));
                        }
                    }
                    (None, None) => {}
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
            if symbol.qualified_name == qualified_method_name
                && let SymbolKind::Function { func_id, .. } = symbol.kind
            {
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
                    s.qualified_name.to_string()
                } else {
                    format!("struct#{}", id)
                }
            }
            Type::Enum(id) => {
                // Check bounds to handle type IDs from imported modules
                if let Some(TypeDefinition::Enum(e)) = self.symbol_table.types.get(*id as usize) {
                    e.qualified_name.to_string()
                } else {
                    format!("enum#{}", id)
                }
            }
            Type::Proto(id) => {
                // Check bounds to handle type IDs from imported modules
                if let Some(TypeDefinition::Proto(p)) = self.symbol_table.types.get(*id as usize) {
                    p.qualified_name.to_string()
                } else {
                    format!("proto#{}", id)
                }
            }
            Type::ProtoGeneric { base, args } => {
                let base_name = if let Some(TypeDefinition::Proto(p)) =
                    self.symbol_table.types.get(*base as usize)
                {
                    p.qualified_name.to_string()
                } else {
                    format!("proto#{}", base)
                };
                let args_str: Vec<String> = args.iter().map(|a| self.type_to_string(a)).collect();
                format!("{}<{}>", base_name, args_str.join(", "))
            }
            Type::BoxType(inner) => format!("box<{}>", self.type_to_string(inner)),
            Type::Nullable(inner) => format!("?{}", self.type_to_string(inner)),
            Type::Function {
                params,
                return_type,
            } => {
                let param_strs: Vec<String> =
                    params.iter().map(|p| self.type_to_string(p)).collect();
                format!(
                    "fn({}) -> {}",
                    param_strs.join(", "),
                    self.type_to_string(return_type)
                )
            }
            Type::Tuple(elements) => {
                let elem_strs: Vec<String> =
                    elements.iter().map(|t| self.type_to_string(t)).collect();
                format!("({})", elem_strs.join(", "))
            }
            Type::Map(k, v) => {
                format!(
                    "HashMap<{}, {}>",
                    self.type_to_string(k),
                    self.type_to_string(v)
                )
            }
        }
    }
}
