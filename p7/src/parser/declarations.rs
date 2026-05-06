use std::ops::Deref;

use crate::ast::{
    Attribute, Effect, EnumVariant, Expression, FunctionDeclaration, Identifier, Parameter,
    ProtoMethod, Statement, StructField, StructMethod, Type,
};
use crate::errors::{ParseError, SourcePos};
use crate::intern::InternedString;
use crate::lexer::{Token, TokenType};

use super::{ParseResult, Parser};

impl Parser {
    fn parse_argument_list(&mut self) -> ParseResult<Vec<Parameter>> {
        let mut parameters = Vec::new();
        self.consume_match(TokenType::OpenParen)?;
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let parameter = match_token! {
                self.peek(),
                TokenType::Ref => {
                    // Receiver shortcut: `ref self` == `self: ref Self`
                    // or `ref mut self` (ephemeral mutable borrow receiver)
                    self.consume(); // consume 'ref'

                    // Check for 'ref mut self'
                    let is_mut = matches!(self.peek().map(|t| &t.token_type), Some(TokenType::Mut));
                    if is_mut {
                        self.consume(); // consume 'mut'
                    }

                    let name = self.parse_identifier()?;
                    if name.name != "self" {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", TokenType::Identifier(name.name)),
                            pos: Some(SourcePos { line: name.line, col: name.col, module: None }),
                        });
                    }

                    let self_type = Type::Identifier(Identifier {
                        name: InternedString::from("Self"),
                        line: name.line,
                        col: name.col,
                    });

                    let arg_type = if is_mut {
                        Type::MutableReference(Box::new(self_type))
                    } else {
                        Type::Reference(Box::new(self_type))
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Box => {
                    // Receiver shortcut: `box self` == `self: box<Self>`
                    self.consume();
                    let name = self.parse_identifier()?;
                    if name.name != "self" {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", TokenType::Identifier(name.name)),
                            pos: Some(SourcePos { line: name.line, col: name.col, module: None }),
                        });
                    }

                    let arg_type = Type::Generic {
                        base: Identifier {
                            name: InternedString::from("box"),
                            line: name.line,
                            col: name.col,
                        },
                        type_args: vec![Type::Identifier(Identifier {
                            name: InternedString::from("Self"),
                            line: name.line,
                            col: name.col,
                        })],
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Identifier(ref ident) if ident == "self" => {
                    // `self` receiver; optional explicit type via `self: ...`.
                    let (line, col) = match self.consume() {
                        Some(t) => (t.line, t.col),
                        None => return Err(ParseError::UnexpectedEof { pos: self.peek_previous().map(|t| SourcePos { line: t.line, col: t.col, module: None }) }),
                    };

                    let name = Identifier { name: InternedString::from("self"), line, col };

                    let arg_type = if self.consume_match(TokenType::Colon).is_ok() {
                        self.parse_type()?
                    } else {
                        Type::Identifier(Identifier { name: InternedString::from("Self"), line, col })
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Identifier(_) => {
                    let name = self.parse_identifier()?;
                    self.consume_match(TokenType::Colon)?;

                    let arg_type = self.parse_type()?;
                    // Optional default value
                    let default_value = if self.consume_match(TokenType::Assignment).is_ok() {
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };
                    Ok(Parameter { name, arg_type, default_value })
                },
            };

            parameters.push(parameter?);

            let _ = self.consume_match(TokenType::Comma);
        }

        Ok(parameters)
    }

    fn parse_attribute(&mut self) -> ParseResult<Attribute> {
        // Expect @ token
        self.consume_match(TokenType::At)?;

        // Parse attribute name (must be an identifier)
        let name = self.parse_identifier()?;

        // Parse arguments (same as struct construction / function call)
        self.consume_match(TokenType::OpenParen)?;
        let mut arguments = Vec::new();

        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            // Parse expression, which might be "name = value" or just "value"
            let expr = self.parse_expression()?;

            // Check if the expression is a named argument (name = value)
            let arg = if let Expression::Binary {
                left,
                operator:
                    Token {
                        token_type: TokenType::Assignment,
                        ..
                    },
                right,
            } = &expr
                && let Expression::Identifier(ident) = left.as_ref()
            {
                (Some(ident.clone()), right.deref().clone())
            } else {
                (None, expr)
            };

            arguments.push(arg);

            // Handle comma separator
            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        Ok(Attribute { name, arguments })
    }

    pub(crate) fn parse_attributes(&mut self) -> ParseResult<Vec<Attribute>> {
        let mut attributes = Vec::new();

        while self.peek_match(TokenType::At) {
            attributes.push(self.parse_attribute()?);
        }

        Ok(attributes)
    }

    pub(crate) fn parse_enum_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Enum)?;

        // Parse optional conformance list: enum[Proto1, Proto2]
        let conformance = if self.peek_match(TokenType::OpenBracket) {
            self.consume();
            let mut protos = vec![];

            // Parse first protocol
            protos.push(self.parse_identifier()?);

            // Parse additional protocols separated by commas
            while self.peek_match(TokenType::Comma) {
                self.consume();
                protos.push(self.parse_identifier()?);
            }

            self.consume_match(TokenType::CloseBracket)?;
            protos
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;

        // New syntax: enum Name( ... ) or enum Name( ... );
        self.consume_match(TokenType::OpenParen)?;

        let mut values = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let variant_name = self.parse_identifier()?;

            // Check if this is a payload variant (has colon)
            let fields = if self.peek_match(TokenType::Colon) {
                self.consume_match(TokenType::Colon)?;

                // Check if we have a tuple type (multi-field payload)
                if self.peek_match(TokenType::OpenParen) {
                    self.consume_match(TokenType::OpenParen)?;
                    let mut field_types = Vec::new();

                    // Parse comma-separated list of types in the tuple
                    while !self.peek_match(TokenType::CloseParen) {
                        field_types.push(self.parse_type()?);

                        if !self.peek_match(TokenType::CloseParen) {
                            self.consume_match(TokenType::Comma)?;
                        }
                    }

                    self.consume_match(TokenType::CloseParen)?;
                    field_types
                } else {
                    // Single-field payload
                    vec![self.parse_type()?]
                }
            } else {
                // Unit variant - no fields
                Vec::new()
            };

            values.push(EnumVariant {
                name: variant_name.name,
                fields,
            });

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        // Check if there's a method block or just a semicolon
        let methods = if self.peek_match(TokenType::OpenBrace) {
            self.consume_match(TokenType::OpenBrace)?;
            let mut methods = Vec::new();

            while !self.peek_match(TokenType::CloseBrace) {
                // Parse attributes for the method
                let attributes = self.parse_attributes()?;
                let is_pub = self.consume_match(TokenType::Pub).is_ok();
                let function = self.parse_function_declaration(attributes, is_pub)?;
                methods.push(StructMethod { is_pub, function });
            }

            self.consume_match(TokenType::CloseBrace)?;
            methods
        } else {
            // No method block, expect semicolon
            self.consume_match(TokenType::Semicolon)?;
            Vec::new()
        };

        Ok(Statement::EnumDeclaration {
            is_pub,
            name,
            attributes,
            conformance,
            type_parameters,
            values,
            methods,
        })
    }

    fn parse_struct_field(&mut self) -> ParseResult<StructField> {
        let is_pub = self.consume_match(TokenType::Pub).is_ok();

        // Try to parse as named field first (identifier followed by colon)
        // We need to check if the next two tokens are identifier and colon
        let is_named = if let Some(token) = self.peek() {
            if matches!(token.token_type, TokenType::Identifier(_)) {
                // Look ahead to see if there's a colon
                if let Some(next_token) = self.peek_ahead(1) {
                    next_token.token_type == TokenType::Colon
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if is_named {
            // Parse named field: name: type [= default]
            let field_name = self.parse_identifier()?;
            self.consume_match(TokenType::Colon)?;
            let field_type = self.parse_type()?;
            let default_value = if self.consume_match(TokenType::Assignment).is_ok() {
                Some(self.parse_expression()?)
            } else {
                None
            };

            Ok(StructField {
                is_pub,
                name: Some(field_name),
                field_type,
                default_value,
            })
        } else {
            // Parse unnamed field (tuple struct): type
            let field_type = self.parse_type()?;

            Ok(StructField {
                is_pub,
                name: None,
                field_type,
                default_value: None,
            })
        }
    }

    fn parse_comma_separated_list<T, F>(&mut self, parse_item: F) -> ParseResult<Vec<T>>
    where
        F: Fn(&mut Self) -> ParseResult<T>,
    {
        self.consume_match(TokenType::OpenParen)?;
        let mut items = Vec::new();

        while let Some(token) = self.peek().cloned() {
            if !items.is_empty() && token.token_type == TokenType::Comma {
                self.consume();
            }

            if let Some(TokenType::CloseParen) = self.peek().map(|t| &t.token_type) {
                self.consume();
                break;
            }

            items.push(parse_item(self)?);
        }

        Ok(items)
    }

    fn parse_struct_method(&mut self) -> ParseResult<StructMethod> {
        // Parse any attributes before the method
        let attributes = self.parse_attributes()?;
        let is_pub = self.consume_match(TokenType::Pub).is_ok();
        let function = self.parse_function_declaration(attributes, is_pub)?;

        Ok(StructMethod { is_pub, function })
    }

    fn parse_struct_field_list(&mut self) -> ParseResult<Vec<StructField>> {
        self.parse_comma_separated_list(|s| s.parse_struct_field())
    }

    fn parse_struct_method_list(&mut self) -> ParseResult<Vec<StructMethod>> {
        self.consume_match(TokenType::OpenBrace)?;

        let mut methods = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            methods.push(self.parse_struct_method()?);
        }

        Ok(methods)
    }

    pub(crate) fn parse_struct_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Struct)?;

        // Parse optional conformance list: struct[Proto1, Proto2]
        let conformance = if self.peek_match(TokenType::OpenBracket) {
            self.consume();
            let mut protos = vec![];

            // Parse first protocol
            protos.push(self.parse_identifier()?);

            // Parse additional protocols separated by commas
            while self.peek_match(TokenType::Comma) {
                self.consume();
                protos.push(self.parse_identifier()?);
            }

            self.consume_match(TokenType::CloseBracket)?;
            protos
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;

        let fields = if self.peek_match(TokenType::OpenParen) {
            self.parse_struct_field_list()?
        } else {
            vec![]
        };

        match_token! {
            self.peek(),
            TokenType::Semicolon => {
                self.consume();
                Ok(Statement::StructDeclaration { is_pub, name, attributes, conformance, type_parameters, fields, methods: vec![] })
            },
            TokenType::OpenBrace => {
                let methods = self.parse_struct_method_list()?;
                Ok(Statement::StructDeclaration { is_pub, name, attributes, conformance, type_parameters, fields, methods })
            },
        }
    }

    fn parse_proto_method(&mut self) -> ParseResult<ProtoMethod> {
        self.consume_match(TokenType::Fn)?;
        let name = self.parse_identifier()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.consume_match(TokenType::Semicolon)?;

        Ok(ProtoMethod {
            name,
            parameters,
            return_type,
        })
    }

    fn parse_proto_method_list(&mut self) -> ParseResult<Vec<ProtoMethod>> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut methods = vec![];

        while !self.peek_match(TokenType::CloseBrace) && !self.peek_match(TokenType::EOF) {
            methods.push(self.parse_proto_method()?);
        }

        self.consume_match(TokenType::CloseBrace)?;
        Ok(methods)
    }

    pub(crate) fn parse_proto_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Proto)?;
        let name = self.parse_identifier()?;

        let methods = if self.peek_match(TokenType::OpenBrace) {
            self.parse_proto_method_list()?
        } else {
            self.consume_match(TokenType::Semicolon)?;
            vec![]
        };

        Ok(Statement::ProtoDeclaration {
            is_pub,
            name,
            attributes,
            methods,
        })
    }

    pub(crate) fn parse_function_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<FunctionDeclaration> {
        self.consume_match(TokenType::Fn)?;

        // Parse effect qualifiers in square brackets after fn: fn[effect1, effect2, ...]
        let effects = if self.peek_match(TokenType::OpenBracket) {
            self.consume(); // consume '['
            let mut effects = vec![];

            while !self.peek_match(TokenType::CloseBracket) && !self.peek_match(TokenType::EOF) {
                // Parse effect identifier (e.g., "throws", "suspend")
                let effect_name = self.parse_identifier()?;

                // Check for parameterized effect: throws<ErrorType>
                if self.peek_match(TokenType::LessThan) {
                    self.consume(); // consume '<'
                    let type_param = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;

                    effects.push(Effect::Parameterized {
                        name: effect_name,
                        type_param,
                    });
                } else {
                    effects.push(Effect::Simple(effect_name));
                }

                // Consume comma if present (for multiple effects)
                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }

            self.consume_match(TokenType::CloseBracket)?;
            effects
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Check if this function has an @intrinsic attribute
        let has_intrinsic = attributes.iter().any(|attr| attr.name.name == "intrinsic");

        // For intrinsic functions, allow semicolon instead of body
        let body = if has_intrinsic && self.peek_match(TokenType::Semicolon) {
            self.consume(); // consume ';'
            vec![] // Empty body for intrinsic functions
        } else {
            self.parse_block()?
        };

        Ok(FunctionDeclaration {
            is_pub,
            name,
            attributes,
            effects,
            type_parameters,
            parameters,
            body,
            return_type,
        })
    }
}
