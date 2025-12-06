use crate::ast::{
    EnumValue, Expression, FunctionCall, FunctionDeclaration, Identifier, NamedPattern, Parameter,
    Pattern, Statement, StatementBlock, StructField, StructInitiation, StructMethod, Type,
};
use crate::errors::{ParseError, SourcePos};
use crate::lexer::{Token, TokenType};

const UNARY_OPERATIONS: &[TokenType] = &[
    TokenType::Not,
    TokenType::Plus,
    TokenType::Minus,
    TokenType::Ampersand,
];

const BINARY_OPTERATORS: &[TokenType] = &[
    TokenType::Assignment,
    TokenType::And,
    TokenType::Or,
    TokenType::Plus,
    TokenType::Minus,
    TokenType::Multiply,
    TokenType::Divide,
    TokenType::Equals,
    TokenType::NotEquals,
    TokenType::GreaterThan,
    TokenType::GreaterThanOrEqual,
    TokenType::LessThan,
    TokenType::LessThanOrEqual,
];

type ParseResult<T> = Result<T, ParseError>;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

macro_rules! match_token {
    ($token:expr, $($pattern:pat $(if $guard:expr)? => $body: expr $(,)?)*) => {
        match $token {
            $(Some(&Token {
                token_type: $pattern,
                ..
            }) $(if $guard)? => $body,)*
            Some(t) => Err(ParseError::UnexpectedToken { found: format!("{:?}", t.token_type), pos: Some(SourcePos { line: t.line, col: t.col }) }),
            _ => Err(ParseError::UnexpectedEof { pos: None }),
        }
    };
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            position: 0,
        }
    }

    fn unconsume(&mut self) {
        if self.position > 0 {
            self.position -= 1;
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn peek_previous(&self) -> Option<&Token> {
        self.tokens.get(self.position.checked_sub(1)?)
    }

    fn ends_with_brace(&self) -> bool {
        matches!(
            self.peek_previous(),
            Some(Token {
                token_type: TokenType::CloseBrace,
                ..
            })
        )
    }

    fn peek_match(&self, token_type: TokenType) -> bool {
        match self.peek() {
            Some(t) => t.token_type.discriminant() == token_type.discriminant(),
            _ => false,
        }
    }

    fn consume_match(&mut self, token_type: TokenType) -> ParseResult<()> {
        match self.peek() {
            Some(t) if t.token_type == token_type => {
                self.consume().unwrap();
                Ok(())
            }
            Some(t) => Err(ParseError::ExpectedToken {
                expected: format!("{:?}", token_type),
                found: format!("{:?}", t.token_type),
                pos: Some(SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            }),
            _ => Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            }),
        }
    }

    fn parse_identifier(&mut self) -> ParseResult<Identifier> {
        match self.consume() {
            Some(Token {
                token_type: TokenType::Identifier(literal),
                line,
                col,
                ..
            }) => Ok(Identifier {
                name: literal.clone(),
                line: *line,
                col: *col,
            }),
            Some(t) => Err(ParseError::ExpectedToken {
                expected: format!("{:?}", TokenType::Identifier("".to_string())),
                found: format!("{:?}", t.token_type),
                pos: Some(SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            }),
            _ => Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            }),
        }
    }

    fn consume(&mut self) -> Option<&Token> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    fn parse_function_call(&mut self, identifier: Identifier) -> ParseResult<Expression> {
        self.consume_match(TokenType::OpenParen)?;
        let mut arguments = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            // Check for named argument: identifier '=' expr
            let arg = if self.peek_match(TokenType::Identifier("".to_string())) {
                let ident = self.parse_identifier()?;
                if self.consume_match(TokenType::Assignment).is_ok() {
                    let expr = self.parse_expression()?;
                    (Some(ident), expr)
                } else {
                    // Not a named argument, treat as positional
                    (None, Expression::Identifier(ident))
                }
            } else {
                (None, self.parse_expression()?)
            };

            arguments.push(arg);
            let _ = self.consume_match(TokenType::Comma);
        }

        Ok(Expression::FunctionCall(FunctionCall {
            name: identifier,
            arguments,
        }))
    }

    fn parse_field_access(&mut self, object: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::Dot)?;
        let field = self.parse_identifier()?;

        if self.peek_match(TokenType::OpenParen) {
            let call_expr = self.parse_function_call(field)?;
            if let Expression::FunctionCall(FunctionCall { name, .. }) = call_expr {
                return Ok(Expression::FieldAccess {
                    object: Box::new(object),
                    field: name,
                });
            } else {
                unreachable!()
            }
        }

        Ok(Expression::FieldAccess {
            object: Box::new(object),
            field,
        })
    }

    fn parse_expression_suffix(&mut self, mut expression: Expression) -> ParseResult<Expression> {
        loop {
            if self.peek_match(TokenType::OpenParen) {
                expression = self.parse_function_call_with_expression(expression)?;
            } else if self.peek_match(TokenType::Dot) {
                expression = self.parse_field_access(expression)?;
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expression> {
        let expression = if let Some(token) = self.peek().cloned() {
            match &token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Expression::IntegerLiteral(*value)
                }
                TokenType::Float(value) => {
                    self.consume();
                    Expression::FloatLiteral(*value)
                }
                TokenType::Identifier(_) => {
                    let identifier = self.parse_identifier()?;
                    Expression::Identifier(identifier)
                }
                TokenType::OpenBrace => {
                    let statements = self.parse_block()?;
                    Expression::Block(statements)
                }
                TokenType::Try => {
                    return self.parse_try_expression();
                }
                TokenType::If => {
                    return self.parse_if_expression();
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        found: format!("{:?}", token.token_type),
                        pos: Some(SourcePos {
                            line: token.line,
                            col: token.col,
                        }),
                    });
                }
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            });
        };

        self.parse_expression_suffix(expression)
    }

    fn parse_function_call_with_expression(
        &mut self,
        identifier: Expression,
    ) -> ParseResult<Expression> {
        self.consume_match(TokenType::OpenParen)?;

        let mut arguments = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            // Check for named argument: identifier '=' expr
            let arg = if self.peek_match(TokenType::Identifier("".to_string())) {
                let ident = self.parse_identifier()?;
                if self.consume_match(TokenType::Assignment).is_ok() {
                    let expr = self.parse_expression()?;
                    (Some(ident), expr)
                } else {
                    // Not a named argument, treat as positional
                    (None, Expression::Identifier(ident))
                }
            } else {
                (None, self.parse_expression()?)
            };

            arguments.push(arg);

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        match identifier.clone() {
            Expression::Identifier(identifier) => Ok(Expression::FunctionCall(FunctionCall {
                name: identifier,
                arguments,
            })),
            Expression::FieldAccess { object, field } => {
                Ok(Expression::FunctionCall(FunctionCall {
                    name: Identifier {
                        name: format!("{}.{}", object.clone().get_name(), field.name),
                        line: field.line,
                        col: field.col,
                    },
                    arguments,
                }))
            }
            _ => Ok(Expression::FunctionCall(FunctionCall {
                name: Identifier {
                    name: identifier.get_name(),
                    line: 0,
                    col: 0,
                },
                arguments,
            })),
        }
    }

    fn parse_unary_expression(&mut self) -> ParseResult<Expression> {
        if let Some(token) = self.peek() {
            if UNARY_OPERATIONS.contains(&token.token_type) {
                let operator = self.consume().unwrap().clone();
                let right = self.parse_unary_expression()?;
                return Ok(Expression::Unary {
                    operator,
                    right: Box::new(right),
                });
            }
        }

        return self.parse_primary_expression();
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_unary_expression()?;
        while let Some(token) = self.peek() {
            if BINARY_OPTERATORS.contains(&token.token_type) {
                let operator = self.consume().unwrap().clone();
                let right = self.parse_primary_expression()?;
                left = Expression::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_if_expression(&mut self) -> ParseResult<Expression> {
        // consume the 'if' token and capture its position for better error reporting
        let if_token = match self.consume() {
            Some(t) if t.token_type == TokenType::If => t.clone(),
            Some(t) => {
                return Err(ParseError::ExpectedToken {
                    expected: format!("{:?}", TokenType::If),
                    found: format!("{:?}", t.token_type),
                    pos: Some(SourcePos {
                        line: t.line,
                        col: t.col,
                    }),
                });
            }
            None => {
                return Err(ParseError::UnexpectedEof {
                    pos: self.peek_previous().map(|t| SourcePos {
                        line: t.line,
                        col: t.col,
                    }),
                });
            }
        };
        let if_pos = (if_token.line, if_token.col);

        let condition = self.parse_expression()?;
        let then_branch = self.parse_expression()?;
        let else_branch = if self.consume_match(TokenType::Else).is_ok() {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(Expression::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            pos: if_pos,
        })
    }

    fn parse_pattern_suffix(&mut self, mut pattern: Pattern) -> ParseResult<Pattern> {
        loop {
            if self.consume_match(TokenType::Dot).is_ok() {
                let field = self.parse_identifier()?;
                pattern = Pattern::FieldAccess {
                    object: Box::new(pattern),
                    field,
                };
            } else {
                break;
            }
        }

        Ok(pattern)
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
                _ => Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", token.token_type),
                    pos: Some(SourcePos {
                        line: token.line,
                        col: token.col,
                    }),
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            })
        }
    }

    fn parse_named_pattern(&mut self) -> ParseResult<NamedPattern> {
        let ident = self.parse_identifier()?;
        let name = if self.consume_match(TokenType::Colon).is_ok() {
            Some(ident)
        } else {
            self.unconsume();
            None
        };

        let pattern = self.parse_pattern()?;
        Ok(NamedPattern { name, pattern })
    }

    fn parse_try_expression(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::Try)?;
        let try_block = self.parse_expression()?;
        let else_block = if self.consume_match(TokenType::Else).is_ok() {
            if self.consume_match(TokenType::OpenBrace).is_ok() {
                let mut statements = vec![];
                loop {
                    let named_pattern = self.parse_named_pattern()?;
                    self.consume_match(TokenType::FatRightArrow)?;

                    let expression = self.parse_expression()?;
                    statements.push(Statement::Branch {
                        named_pattern,
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

                Some(Box::new(Expression::Block(statements)))
            } else {
                Some(Box::new(self.parse_expression()?))
            }
        } else {
            None
        };

        Ok(Expression::Try {
            try_block: Box::new(try_block),
            else_block,
        })
    }

    fn parse_block(&mut self) -> ParseResult<Vec<Statement>> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut statements = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            statements.push(self.parse_statement()?);
        }

        return Ok(statements);
    }

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
                TokenType::Ampersand => {
                    // &self: no default allowed
                    self.consume();
                    self.consume_match(TokenType::Identifier("self".to_string()))?;
                    Ok(Parameter {
                        name: Identifier { name: "self".to_string(), line: 0, col: 0 },
                        arg_type: Type::Reference(Box::new(Type::Identifier(Identifier { name: "Self".to_string(), line: 0, col: 0 }))),
                        default_value: None,
                     })
                },
                TokenType::Identifier(ref ident) if ident == "self" => {
                    // self: no default allowed
                    self.consume();
                    Ok(Parameter {
                        name: Identifier { name: "self".to_string(), line: 0, col: 0 },
                        arg_type: Type::Identifier(Identifier { name: "Self".to_string(), line: 0, col: 0 }),
                        default_value: None,
                    })
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

    fn parse_enum_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Enum)?;
        let name = self.parse_identifier()?;

        self.consume_match(TokenType::OpenBrace)?;

        let mut values = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            let literal = self.parse_identifier()?;
            values.push(EnumValue { name: literal.name });

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseBrace) {
                comma?;
            }
        }

        Ok(Statement::EnumDeclaration { name, values })
    }

    fn parse_struct_field(&mut self) -> ParseResult<StructField> {
        let is_pub = self.consume_match(TokenType::Pub).is_ok();

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
            name: field_name,
            field_type,
            default_value,
        })
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
        let is_pub = self.consume_match(TokenType::Pub).is_ok();
        let function = self.parse_function_declaration()?;

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

    fn parse_struct_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Struct)?;
        let name = self.parse_identifier()?;

        let fields = if self.peek_match(TokenType::OpenParen) {
            self.parse_struct_field_list()?
        } else {
            vec![]
        };

        match_token! {
            self.peek(),
            TokenType::Semicolon => {
                self.consume();
                return Ok(Statement::StructDeclaration { name, fields, methods: vec![] });
            },
            TokenType::OpenBrace => {
                let methods = self.parse_struct_method_list()?;
                return Ok(Statement::StructDeclaration { name, fields, methods });
            },
        }
    }

    fn parse_function_declaration(&mut self) -> ParseResult<FunctionDeclaration> {
        self.consume_match(TokenType::Fn)?;
        let mut effects = vec![];
        if self.consume_match(TokenType::OpenBracket).is_ok() {
            loop {
                effects.push(self.parse_identifier()?);

                let comma = self.consume_match(TokenType::Comma);
                if !self.peek_match(TokenType::CloseBracket) {
                    comma?;
                } else {
                    break;
                }
            }

            self.consume_match(TokenType::CloseBracket)?;
        }

        let name = self.parse_identifier()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(FunctionDeclaration {
            name,
            effects,
            parameters,
            body,
            return_type,
        })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        if let Some(token) = self.peek() {
            match &token.token_type {
                TokenType::Ampersand => {
                    self.consume();
                    let ty = self.parse_type()?;
                    Ok(Type::Reference(Box::new(ty)))
                }
                TokenType::OpenBracket => {
                    self.consume();
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::CloseBracket)?;
                    Ok(Type::Array(Box::new(ty)))
                }
                TokenType::Identifier(_) => {
                    let ident = self.parse_identifier()?;
                    Ok(Type::Identifier(ident))
                }
                _ => Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", token.token_type),
                    pos: Some(SourcePos {
                        line: token.line,
                        col: token.col,
                    }),
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                }),
            })
        }
    }

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Fn) => self
                .parse_function_declaration()
                .map(Statement::FunctionDeclaration),
            Some(TokenType::Enum) => self.parse_enum_declaration(),
            Some(TokenType::Struct) => self.parse_struct_declaration(),
            // Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => {
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Return(Box::new(expr)))
            }
            Some(TokenType::Throw) => {
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Throw(expr))
            }
            Some(TokenType::Let) => {
                self.consume();

                let identifier = self.parse_identifier()?;
                self.consume_match(TokenType::Assignment)?;
                let expression = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;

                Ok(Statement::Let {
                    identifier,
                    expression,
                })
            }
            _ => {
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

    pub fn parse(&mut self) -> ParseResult<Vec<Statement>> {
        let mut statements = Vec::new();

        while self.peek().is_some() {
            statements.push(self.parse_statement()?);
        }

        Ok(statements)
    }
}
