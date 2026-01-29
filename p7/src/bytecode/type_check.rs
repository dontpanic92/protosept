use crate::errors::SourcePos;
use crate::semantic::{PrimitiveType, Type, TypeId, UserDefinedType};
use crate::errors::SemanticError;
use crate::ast::Type as ParsedType;

use super::codegen::{Generator, SaResult};

impl Generator {
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
                if let Some(ty) = self.symbol_table.find_type_in_scope(&identifier.name) {
                    Ok(ty)
                } else {
                    Err(SemanticError::TypeNotFound {
                        name: identifier.name.clone(),
                        pos: Some(SourcePos {
                            line: identifier.line,
                            col: identifier.col,
                        }),
                    })
                }
            }
            ParsedType::Reference(r) => {
                let ty = self.get_semantic_type(r)?;
                Ok(Type::Reference(Box::new(ty)))
            }
            ParsedType::Array(a) => {
                let ty = self.get_semantic_type(a)?;
                Ok(Type::Array(Box::new(ty)))
            }
            ParsedType::Generic { base, type_args } => {
                // Handle box<T> specially (builtin generic type)
                if base.name == "box" {
                    if type_args.len() != 1 {
                        return Err(SemanticError::Other(format!(
                            "box<T> requires exactly one type argument, found {} at line {} column {}",
                            type_args.len(), base.line, base.col
                        )));
                    }
                    let inner_ty = self.get_semantic_type(&type_args[0])?;
                    return Ok(Type::BoxType(Box::new(inner_ty)));
                }
                
                // Implement proper generic type resolution with monomorphization
                
                // First, find the base generic type
                let base_type = if let Some(ty) = self.symbol_table.find_type_in_scope(&base.name) {
                    ty
                } else {
                    return Err(SemanticError::TypeNotFound {
                        name: base.name.clone(),
                        pos: Some(SourcePos {
                            line: base.line,
                            col: base.col,
                        }),
                    });
                };
                
                // Resolve all type arguments
                let mut resolved_type_args = Vec::new();
                for arg in type_args {
                    resolved_type_args.push(self.get_semantic_type(arg)?);
                }
                
                // For now, only handle struct and enum monomorphization
                match base_type {
                    Type::Struct(base_type_id) => {
                        self.monomorphize_struct(base_type_id, resolved_type_args, &base.name, base.line, base.col)
                    }
                    Type::Enum(base_type_id) => {
                        self.monomorphize_enum(base_type_id, resolved_type_args, &base.name, base.line, base.col)
                    }
                    _ => {
                        // For non-struct/enum types, just return the base type for now
                        // TODO: implement monomorphization for other types if needed
                        Ok(base_type)
                    }
                }
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
            UserDefinedType::Struct(s) => ("Struct", s.qualified_name.clone()),
            UserDefinedType::Enum(e) => ("Enum", e.qualified_name.clone()),
            _ => return Err(SemanticError::Other("Expected struct or enum type".to_string())),
        };
        
        // For each protocol the type claims to conform to
        for &proto_id in conforming_to {
            let proto = match &self.symbol_table.types[proto_id as usize] {
                UserDefinedType::Proto(p) => p,
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
                        method_name, type_name, qualified_name, method_params.len(),
                        proto.qualified_name, param_types.len(), line, col
                    )));
                }
                
                // Check each parameter type matches
                // Special handling for first parameter (self) which should be ref to the type/proto type
                for (i, (expected_type, actual_type)) in param_types.iter().zip(method_params.iter()).enumerate() {
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
                            method_name, type_name, qualified_name, i,
                            self.type_to_string(actual_type),
                            proto.qualified_name,
                            self.type_to_string(expected_type),
                            line, col
                        )));
                    }
                }
                
                // Check return type matches
                match (return_type, method_return_type) {
                    (Some(expected), Some(actual)) => {
                        // Both unit and no return type should be considered equivalent
                        let expected_is_unit = matches!(expected, Type::Primitive(PrimitiveType::Unit));
                        let actual_is_unit = matches!(actual, Type::Primitive(PrimitiveType::Unit));
                        
                        if expected_is_unit && actual_is_unit {
                            // Both return unit, this is fine
                        } else if !self.types_equal(&expected, &actual) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name, type_name, qualified_name,
                                self.type_to_string(&actual),
                                proto.qualified_name,
                                self.type_to_string(&expected),
                                line, col
                            )));
                        }
                    }
                    (Some(expected), None) => {
                        // Proto requires a return type, but type method returns nothing (unit)
                        // If proto expects unit, this is fine
                        if !matches!(expected, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns nothing, but protocol '{}' requires return type '{}' at line {} column {}",
                                method_name, type_name, qualified_name,
                                proto.qualified_name,
                                self.type_to_string(&expected),
                                line, col
                            )));
                        }
                    }
                    (None, Some(actual)) => {
                        // Proto expects no return, but type returns something
                        // If type returns unit, this is fine
                        if !matches!(actual, Type::Primitive(PrimitiveType::Unit)) {
                            return Err(SemanticError::Other(format!(
                                "Method '{}' in {} '{}' returns type '{}', but protocol '{}' expects no return type at line {} column {}",
                                method_name, type_name, qualified_name,
                                self.type_to_string(&actual),
                                proto.qualified_name,
                                line, col
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
    pub(super) fn find_type_method(&self, type_id: TypeId, method_name: &str) -> Option<(Vec<Type>, Option<Type>)> {
        // Search for a function with the qualified name type_name.method_name
        let qualified_name = match &self.symbol_table.types[type_id as usize] {
            UserDefinedType::Struct(s) => s.qualified_name.clone(),
            UserDefinedType::Enum(e) => e.qualified_name.clone(),
            _ => return None,
        };
        
        let qualified_method_name = format!("{}.{}", qualified_name, method_name);
        
        // Look through all symbols to find the method
        for symbol in &self.symbol_table.symbols {
            if symbol.qualified_name == qualified_method_name {
                if let crate::semantic::SymbolKind::Function { type_id, .. } = symbol.kind {
                    // Get the function from the UserDefinedType
                    if let UserDefinedType::Function(func) = &self.symbol_table.types[type_id as usize] {
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
        }
        
        None
    }
    
    /// Helper to check if two types are equal
    pub(super) fn types_equal(&self, a: &Type, b: &Type) -> bool {
        match (a, b) {
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => self.types_equal(a, b),
            (Type::Reference(a), Type::Reference(b)) => self.types_equal(a, b),
            (Type::Struct(a), Type::Struct(b)) => a == b,
            (Type::Enum(a), Type::Enum(b)) => a == b,
            (Type::Proto(a), Type::Proto(b)) => a == b,
            (Type::BoxType(a), Type::BoxType(b)) => self.types_equal(a, b),
            _ => false,
        }
    }
    
    /// Helper to convert a type to a string for error messages
    pub(super) fn type_to_string(&self, ty: &Type) -> String {
        match ty {
            Type::Primitive(p) => format!("{:?}", p).to_lowercase(),
            Type::Array(inner) => format!("array<{}>", self.type_to_string(inner)),
            Type::Reference(inner) => format!("ref {}", self.type_to_string(inner)),
            Type::Struct(id) => {
                if let UserDefinedType::Struct(s) = &self.symbol_table.types[*id as usize] {
                    s.qualified_name.clone()
                } else {
                    format!("struct#{}", id)
                }
            }
            Type::Enum(id) => {
                if let UserDefinedType::Enum(e) = &self.symbol_table.types[*id as usize] {
                    e.qualified_name.clone()
                } else {
                    format!("enum#{}", id)
                }
            }
            Type::Proto(id) => {
                if let UserDefinedType::Proto(p) = &self.symbol_table.types[*id as usize] {
                    p.qualified_name.clone()
                } else {
                    format!("proto#{}", id)
                }
            }
            Type::BoxType(inner) => format!("box<{}>", self.type_to_string(inner)),
            Type::Function(id) => {
                if let UserDefinedType::Function(f) = &self.symbol_table.types[*id as usize] {
                    f.qualified_name.clone()
                } else {
                    format!("function#{}", id)
                }
            }
        }
    }
}
