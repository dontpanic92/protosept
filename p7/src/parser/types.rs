use crate::ast::{Identifier, Type, TypeParameter};
use crate::errors::{ParseError, SourcePos};
use crate::intern::InternedString;
use crate::lexer::TokenType;

use super::{ParseResult, Parser};

impl Parser {
    pub(crate) fn parse_type(&mut self) -> ParseResult<Type> {
        if let Some(token) = self.peek() {
            match &token.token_type {
                TokenType::Question => {
                    self.consume();
                    // ?Type syntax
                    let ty = self.parse_type()?;
                    Ok(Type::Nullable(Box::new(ty)))
                }
                TokenType::Ref => {
                    self.consume();
                    // ref<Type> syntax - must have angle brackets
                    self.consume_match(TokenType::LessThan)?;
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;
                    Ok(Type::Reference(Box::new(ty)))
                }
                TokenType::Box => {
                    let (line, col) = (token.line, token.col);
                    self.consume();
                    // box<Type> syntax - must have angle brackets
                    self.consume_match(TokenType::LessThan)?;
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;
                    Ok(Type::Generic {
                        base: Identifier {
                            name: InternedString::from("box"),
                            line,
                            col,
                        },
                        type_args: vec![ty],
                    })
                }
                TokenType::OpenBracket => {
                    self.consume();
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::CloseBracket)?;
                    Ok(Type::Array(Box::new(ty)))
                }
                TokenType::OpenParen => {
                    // Tuple type: (T1, T2, ...) or parenthesized type: (T)
                    self.consume(); // consume '('
                    let first_ty = self.parse_type()?;
                    if self.peek_match(TokenType::Comma) {
                        // Tuple type: (T1, T2, ...)
                        self.consume(); // consume ','
                        let mut types = vec![first_ty];
                        while !self.peek_match(TokenType::CloseParen) {
                            types.push(self.parse_type()?);
                            if self.peek_match(TokenType::Comma) {
                                self.consume();
                            } else if !self.peek_match(TokenType::CloseParen) {
                                return Err(ParseError::UnexpectedToken {
                                    found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                                    pos: self.peek().map(|t| SourcePos {
                                        line: t.line,
                                        col: t.col,
                                        module: None,
                                    }),
                                });
                            }
                        }
                        self.consume_match(TokenType::CloseParen)?;
                        Ok(Type::Tuple(types))
                    } else {
                        // Parenthesized type (grouping): (T)
                        self.consume_match(TokenType::CloseParen)?;
                        Ok(first_ty)
                    }
                }
                TokenType::Fn => {
                    // fn(T1, T2) -> R function type
                    self.consume(); // consume 'fn'
                    self.consume_match(TokenType::OpenParen)?;
                    let mut param_types = Vec::new();
                    if !self.peek_match(TokenType::CloseParen) {
                        param_types.push(self.parse_type()?);
                        while self.consume_match(TokenType::Comma).is_ok() {
                            param_types.push(self.parse_type()?);
                        }
                    }
                    self.consume_match(TokenType::CloseParen)?;
                    let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
                        self.parse_type()?
                    } else {
                        Type::Identifier(Identifier {
                            name: InternedString::from("unit"),
                            line: 0,
                            col: 0,
                        })
                    };
                    Ok(Type::Function {
                        param_types,
                        return_type: Box::new(return_type),
                    })
                }
                TokenType::Identifier(_) => {
                    let mut ident = self.parse_identifier()?;

                    // Support module-qualified types like `ui.Message`
                    if self.peek_match(TokenType::Dot) {
                        let mut full_name = ident.name.to_string();
                        let line = ident.line;
                        let col = ident.col;

                        while self.peek_match(TokenType::Dot) {
                            self.consume(); // consume '.'
                            let next = self.parse_identifier()?;
                            full_name.push('.');
                            full_name.push_str(&next.name);
                        }

                        ident = Identifier {
                            name: InternedString::from(full_name),
                            line,
                            col,
                        };
                    }

                    // Check for generic type syntax: identifier<type_args>
                    if self.peek_match(TokenType::LessThan) {
                        self.consume(); // consume '<'

                        // Handle empty type argument list: identifier<>
                        if self.peek_match(TokenType::GreaterThan) {
                            self.consume(); // consume '>'
                            return Err(ParseError::UnexpectedToken {
                                found: "empty type argument list".to_string(),
                                pos: Some(SourcePos {
                                    line: ident.line,
                                    col: ident.col,
                                    module: None,
                                }),
                            });
                        }

                        let mut type_args = vec![];

                        loop {
                            type_args.push(self.parse_type()?);

                            if self.peek_match(TokenType::Comma) {
                                self.consume(); // consume ','
                            } else {
                                break;
                            }
                        }

                        self.consume_match(TokenType::GreaterThan)?; // consume '>'

                        Ok(Type::Generic {
                            base: ident,
                            type_args,
                        })
                    } else {
                        Ok(Type::Identifier(ident))
                    }
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

    /// Try to parse type arguments in expression context (e.g., Container<int>)
    /// Returns Ok with type args if successful, Err otherwise.
    ///
    /// This is used to disambiguate between generic instantiation (`Container<int>`)
    /// and comparison operators (`a < b`). The parser saves its position and attempts
    /// to parse type arguments. If successful, it's a generic instantiation; if it fails,
    /// the parser backtracks to the saved position and treats `<` as a comparison operator.
    pub(crate) fn try_parse_type_arguments(&mut self) -> ParseResult<Vec<Type>> {
        // Save parser state for potential backtracking
        let saved_pos = self.position;

        // Consume the '<'
        if !self.peek_match(TokenType::LessThan) {
            return Err(ParseError::UnexpectedToken {
                found: "not <".to_string(),
                pos: None,
            });
        }
        self.consume();

        // Check for empty type argument list: identifier<>
        // This is not allowed (consistent with parse_type behavior)
        if self.peek_match(TokenType::GreaterThan) {
            self.position = saved_pos;
            return Err(ParseError::UnexpectedToken {
                found: "empty type argument list".to_string(),
                pos: None,
            });
        }

        // Try to parse type arguments
        let mut type_args = vec![];

        loop {
            // Try to parse a type
            match self.parse_type() {
                Ok(ty) => type_args.push(ty),
                Err(_) => {
                    // Failed to parse type - this might be a comparison operator
                    // Restore position and fail gracefully
                    self.position = saved_pos;
                    return Err(ParseError::UnexpectedToken {
                        found: "not a type argument".to_string(),
                        pos: None,
                    });
                }
            }

            if self.peek_match(TokenType::Comma) {
                self.consume();
            } else {
                break;
            }
        }

        // Must end with '>'
        if !self.peek_match(TokenType::GreaterThan) {
            self.position = saved_pos;
            return Err(ParseError::UnexpectedToken {
                found: "expected '>' to close type arguments".to_string(),
                pos: None,
            });
        }
        self.consume();

        Ok(type_args)
    }

    pub(crate) fn parse_type_parameters(&mut self) -> ParseResult<Vec<TypeParameter>> {
        if !self.peek_match(TokenType::LessThan) {
            return Ok(vec![]);
        }

        self.consume(); // consume '<'

        // Handle empty type parameter list: <>
        if self.peek_match(TokenType::GreaterThan) {
            self.consume(); // consume '>'
            return Ok(vec![]);
        }

        let mut type_params = vec![];

        loop {
            let name = self.parse_identifier()?;

            // Check for bounds: T: Proto1 + Proto2 + ...
            let bounds = if self.peek_match(TokenType::Colon) {
                self.consume(); // consume ':'
                let mut bounds = vec![self.parse_identifier()?];
                while self.peek_match(TokenType::Plus) {
                    self.consume(); // consume '+'
                    let bound = self.parse_identifier()?;
                    // Check for duplicate bounds (spec §20.5: listing same proto twice is ERROR)
                    if bounds.iter().any(|b| b.name == bound.name) {
                        return Err(ParseError::UnexpectedToken {
                            found: format!(
                                "duplicate bound '{}' on type parameter '{}'",
                                bound.name, name.name
                            ),
                            pos: Some(SourcePos {
                                line: bound.line,
                                col: bound.col,
                                module: None,
                            }),
                        });
                    }
                    bounds.push(bound);
                }
                bounds
            } else {
                vec![]
            };

            type_params.push(TypeParameter { name, bounds });

            if self.peek_match(TokenType::Comma) {
                self.consume(); // consume ','
            } else {
                break;
            }
        }

        self.consume_match(TokenType::GreaterThan)?; // consume '>'
        Ok(type_params)
    }
}
