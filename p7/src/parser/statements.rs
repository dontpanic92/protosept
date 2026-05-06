use crate::ast::{Expression, Identifier, NamedPattern, Pattern, Statement};
use crate::errors::{ParseError, SourcePos};
use crate::intern::InternedString;
use crate::lexer::{Token, TokenType};

use super::{ParseResult, Parser};

impl Parser {
    fn parse_pattern_suffix(&mut self, mut pattern: Pattern) -> ParseResult<Pattern> {
        loop {
            if self.consume_match(TokenType::Dot).is_ok() {
                let field = self.parse_identifier()?;
                pattern = Pattern::FieldAccess {
                    object: Box::new(pattern),
                    field,
                };
            } else if self.peek_match(TokenType::OpenParen) {
                // Check if this is a destructuring pattern: Ident(...) or Ident.Ident(...)
                match &pattern {
                    Pattern::FieldAccess { object, field } => {
                        // EnumName.Variant(...) pattern
                        if let Pattern::Identifier(enum_name) = object.as_ref() {
                            let enum_name = enum_name.clone();
                            let variant_name = field.clone();
                            self.consume_match(TokenType::OpenParen)?;
                            let sub_patterns = self.parse_sub_patterns()?;
                            self.consume_match(TokenType::CloseParen)?;
                            pattern = Pattern::EnumVariant {
                                enum_name,
                                variant_name,
                                sub_patterns,
                            };
                        } else if let Pattern::FieldAccess {
                            object: inner_obj,
                            field: inner_field,
                        } = object.as_ref()
                            && let Pattern::Identifier(module_name) = inner_obj.as_ref()
                        {
                            // module.EnumName.Variant(...) pattern
                            let qualified_name = Identifier {
                                name: InternedString::from(format!(
                                    "{}.{}",
                                    module_name.name, inner_field.name
                                )),
                                line: module_name.line,
                                col: module_name.col,
                            };
                            let variant_name = field.clone();
                            self.consume_match(TokenType::OpenParen)?;
                            let sub_patterns = self.parse_sub_patterns()?;
                            self.consume_match(TokenType::CloseParen)?;
                            pattern = Pattern::EnumVariant {
                                enum_name: qualified_name,
                                variant_name,
                                sub_patterns,
                            };
                        } else {
                            break;
                        }
                    }
                    Pattern::Identifier(struct_name) => {
                        // StructName(...) pattern
                        let struct_name = struct_name.clone();
                        self.consume_match(TokenType::OpenParen)?;
                        let field_patterns = self.parse_sub_patterns()?;
                        self.consume_match(TokenType::CloseParen)?;
                        pattern = Pattern::StructPattern {
                            struct_name,
                            field_patterns,
                        };
                    }
                    _ => break,
                }
            } else {
                break;
            }
        }

        Ok(pattern)
    }

    pub(crate) fn parse_sub_patterns(&mut self) -> ParseResult<Vec<Pattern>> {
        let mut patterns = Vec::new();
        if self.peek_match(TokenType::CloseParen) {
            return Ok(patterns);
        }
        loop {
            patterns.push(self.parse_pattern()?);
            if self.consume_match(TokenType::Comma).is_err() {
                break;
            }
            // Allow trailing comma
            if self.peek_match(TokenType::CloseParen) {
                break;
            }
        }
        Ok(patterns)
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        if let Some(token) = self.peek().cloned() {
            match &token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Ok(Pattern::IntegerLiteral(*value))
                }
                TokenType::Float(value) => {
                    self.consume();
                    Ok(Pattern::FloatLiteral(*value))
                }
                TokenType::StringLiteral(value) => {
                    self.consume();
                    Ok(Pattern::StringLiteral(value.clone()))
                }
                TokenType::Identifier(_) => {
                    let identifier = self.parse_identifier()?;
                    let pattern = Pattern::Identifier(identifier);
                    let pattern = self.parse_pattern_suffix(pattern)?;
                    Ok(pattern)
                }
                TokenType::OpenParen => {
                    // Tuple pattern: (p1, p2, ...)
                    self.consume(); // consume '('
                    let sub_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    Ok(Pattern::TuplePattern { sub_patterns })
                }
                _ => Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", token.token_type),
                    pos: Some(SourcePos {
                        line: token.line,
                        col: token.col,
                        module: None,
                    }),
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            })
        }
    }

    /// Helper method to check if the current position has a named pattern (identifier followed by colon)
    fn has_named_pattern_binding(&mut self) -> bool {
        if let Some(token) = self.peek() {
            if matches!(token.token_type, TokenType::Identifier(_)) {
                // Look ahead to see if there's a colon after the identifier
                let saved_pos = self.position;
                let _ = self.parse_identifier();
                let has_colon = self
                    .peek()
                    .map(|t| t.token_type == TokenType::Colon)
                    .unwrap_or(false);
                self.position = saved_pos; // Restore position
                has_colon
            } else {
                false
            }
        } else {
            false
        }
    }

    fn parse_named_pattern(&mut self) -> ParseResult<NamedPattern> {
        // Try to parse as "name: pattern" first
        let name = if self.has_named_pattern_binding() {
            let ident = self.parse_identifier()?;
            self.consume_match(TokenType::Colon)?;
            Some(ident)
        } else {
            None
        };

        let pattern = self.parse_pattern()?;
        Ok(NamedPattern { name, pattern })
    }

    pub(crate) fn parse_try_expression(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::Try)?;
        let try_block = self.parse_expression()?;
        let else_arms = if self.consume_match(TokenType::Else).is_ok() {
            if self.consume_match(TokenType::OpenBrace).is_ok() {
                let mut arms = vec![];
                loop {
                    // Check if we've reached the end
                    if self.consume_match(TokenType::CloseBrace).is_ok() {
                        break;
                    }

                    let pattern = self.parse_named_pattern()?;
                    self.consume_match(TokenType::FatRightArrow)?;
                    let expression = self.parse_expression()?;

                    arms.push(crate::ast::MatchArm {
                        pattern,
                        expression,
                    });

                    let ends_with_brace = self.ends_with_brace();
                    let comma = self.consume_match(TokenType::Comma);
                    if !ends_with_brace {
                        comma?;
                    }

                    if self.consume_match(TokenType::CloseBrace).is_ok() {
                        break;
                    }
                }
                arms
            } else {
                // Single expression else: `try expr else fallback`
                // Treat as a wildcard arm that matches anything
                let pattern = NamedPattern {
                    name: None,
                    pattern: Pattern::Identifier(Identifier {
                        name: InternedString::from("_"),
                        line: 0,
                        col: 0,
                    }),
                };
                let expression = self.parse_expression()?;
                vec![crate::ast::MatchArm {
                    pattern,
                    expression,
                }]
            }
        } else {
            vec![]
        };

        Ok(Expression::Try {
            try_block: Box::new(try_block),
            else_arms,
        })
    }

    pub(crate) fn parse_match_expression(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::Match)?;
        let scrutinee = self.parse_expression()?;
        self.consume_match(TokenType::OpenBrace)?;

        let mut arms = vec![];
        loop {
            // Check if we've reached the end of the match expression
            if self.consume_match(TokenType::CloseBrace).is_ok() {
                break;
            }

            // Parse pattern => expression
            let pattern = self.parse_named_pattern()?;
            self.consume_match(TokenType::FatRightArrow)?;
            let expression = self.parse_expression()?;

            arms.push(crate::ast::MatchArm {
                pattern,
                expression,
            });

            // Handle optional comma
            let ends_with_brace = self.ends_with_brace();
            let comma = self.consume_match(TokenType::Comma);
            if !ends_with_brace {
                comma?;
            }

            // Check for closing brace again
            if self.consume_match(TokenType::CloseBrace).is_ok() {
                break;
            }
        }

        Ok(Expression::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        })
    }

    pub(crate) fn parse_block(&mut self) -> ParseResult<Vec<Statement>> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut statements = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            statements.push(self.parse_statement()?);
        }

        Ok(statements)
    }

    fn parse_import_statement(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Import)?;

        // Parse the module path (can be dotted identifier or relative path starting with .)
        let mut module_path = String::new();

        // Check if it's a relative import starting with .
        if self.peek_match(TokenType::Dot) {
            module_path.push('.');
            self.consume();
        }

        // Parse the rest of the path
        loop {
            match self.peek() {
                Some(Token {
                    token_type: TokenType::Identifier(id),
                    ..
                }) => {
                    module_path.push_str(id);
                    self.consume();

                    // Check for another dot
                    if self.peek_match(TokenType::Dot) {
                        module_path.push('.');
                        self.consume();
                    } else {
                        break;
                    }
                }
                _ => {
                    // If we haven't parsed any identifiers or if the path ends with a dot, it's an error
                    if module_path.ends_with('.') {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                            pos: self.peek().map(|t| SourcePos {
                                line: t.line,
                                col: t.col,
                                module: None,
                            }),
                        });
                    }
                    break;
                }
            }
        }

        // Check for optional "as" alias
        let alias = if self.peek_match(TokenType::As) {
            self.consume();
            Some(self.parse_identifier()?.name)
        } else {
            None
        };

        self.consume_match(TokenType::Semicolon)?;

        Ok(Statement::Import {
            module_path: InternedString::from(module_path),
            alias,
        })
    }

    pub(crate) fn parse_statement(&mut self) -> ParseResult<Statement> {
        // Parse attributes first (they come before pub in the syntax)
        let attributes = self.parse_attributes()?;

        // Then, check for pub keyword
        let is_pub = if self.peek_match(TokenType::Pub) {
            self.consume();
            true
        } else {
            false
        };

        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Import) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on import statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on import statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                self.parse_import_statement()
            }
            Some(TokenType::Fn) => self
                .parse_function_declaration(attributes, is_pub)
                .map(Statement::FunctionDeclaration),
            Some(TokenType::Enum) => self.parse_enum_declaration(attributes, is_pub),
            Some(TokenType::Struct) => self.parse_struct_declaration(attributes, is_pub),
            Some(TokenType::Proto) => self.parse_proto_declaration(attributes, is_pub),
            // Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on return statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on return statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                let pos = self
                    .peek()
                    .map(|t| SourcePos {
                        line: t.line,
                        col: t.col,
                        module: None,
                    })
                    .unwrap_or(SourcePos {
                        line: 0,
                        col: 0,
                        module: None,
                    });
                self.consume();
                // Support bare `return;` (unit return) by checking for semicolon
                if self.peek().map(|t| &t.token_type) == Some(&TokenType::Semicolon) {
                    self.consume(); // consume the semicolon
                    Ok(Statement::Return {
                        expression: None,
                        pos,
                    })
                } else {
                    let expr = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    Ok(Statement::Return {
                        expression: Some(Box::new(expr)),
                        pos,
                    })
                }
            }
            Some(TokenType::Throw) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on throw statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on throw statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Throw(expr))
            }
            Some(TokenType::Let) => {
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on let statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }

                self.consume(); // consume 'let'
                let is_mutable = matches!(self.peek().map(|t| &t.token_type), Some(TokenType::Mut));
                if is_mutable {
                    self.consume(); // consume 'mut'
                }

                // Check for tuple destructuring: let (a, b) = ... or var (x, y) = ...
                if self.peek_match(TokenType::OpenParen) {
                    self.consume_match(TokenType::OpenParen)?;
                    let sub_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    self.consume_match(TokenType::Assignment)?;
                    let expression = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    return Ok(Statement::LetDestructure {
                        is_mutable,
                        pattern: Pattern::TuplePattern { sub_patterns },
                        expression,
                    });
                }

                let identifier = self.parse_identifier()?;

                // Check for destructuring pattern: let Pos(r, c) = ...
                if self.peek_match(TokenType::OpenParen) {
                    let struct_name = identifier;
                    self.consume_match(TokenType::OpenParen)?;
                    let field_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    self.consume_match(TokenType::Assignment)?;
                    let expression = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    return Ok(Statement::LetDestructure {
                        is_mutable,
                        pattern: Pattern::StructPattern {
                            struct_name,
                            field_patterns,
                        },
                        expression,
                    });
                }

                // Check for enum destructuring: let Result.Ok(n) = ...
                if self.peek_match(TokenType::Dot) {
                    let saved_pos = self.position;
                    self.consume(); // consume '.'
                    if let Ok(variant_name) = self.parse_identifier()
                        && self.peek_match(TokenType::OpenParen)
                    {
                        let enum_name = identifier;
                        self.consume_match(TokenType::OpenParen)?;
                        let sub_patterns = self.parse_sub_patterns()?;
                        self.consume_match(TokenType::CloseParen)?;
                        self.consume_match(TokenType::Assignment)?;
                        let expression = self.parse_expression()?;
                        self.consume_match(TokenType::Semicolon)?;
                        return Ok(Statement::LetDestructure {
                            is_mutable,
                            pattern: Pattern::EnumVariant {
                                enum_name,
                                variant_name,
                                sub_patterns,
                            },
                            expression,
                        });
                    }
                    self.position = saved_pos;
                }

                // Check for optional type annotation: let p: Type = ...
                let type_annotation = if self.peek_match(TokenType::Colon) {
                    self.consume(); // consume ':'
                    Some(self.parse_type()?)
                } else {
                    None
                };

                self.consume_match(TokenType::Assignment)?;
                let expression = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;

                Ok(Statement::Let {
                    is_pub,
                    is_mutable,
                    identifier,
                    type_annotation,
                    expression,
                })
            }
            _ => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on expression statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on expression statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                let expression = self.parse_expression()?;
                let ends_with_brace = self.ends_with_brace();

                match_token! {
                    self.peek(),
                    TokenType::Semicolon => {
                        self.consume();
                        Ok(Statement::Expression(expression))
                    }
                    TokenType::CloseBrace => Ok(Statement::Expression(
                        Expression::BlockValue(Box::new(expression)),
                    )),
                    _ if ends_with_brace => {
                        Ok(Statement::Expression(expression))
                    },
                }
            }
        }
    }
}
