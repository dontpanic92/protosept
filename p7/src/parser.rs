use std::ops::Deref;

use crate::ast::{
    Attribute, EnumValue, Expression, FunctionCall, FunctionDeclaration, Identifier, NamedPattern,
    Parameter, Pattern, ProtoMethod, Statement, StructField, StructMethod, Type,
};
use crate::errors::{ParseError, SourcePos};
use crate::lexer::{Token, TokenType};

const UNARY_OPERATIONS: &[TokenType] = &[
    TokenType::Not,
    TokenType::Plus,
    TokenType::Minus,
    TokenType::Multiply, // unary `*` for deref of `ref T`
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

    fn parse_field_access(&mut self, object: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::Dot)?;
        let field = self.parse_identifier()?;

        Ok(Expression::FieldAccess {
            object: Box::new(object),
            field,
        })
    }

    fn parse_expression_suffix(&mut self, mut expression: Expression) -> ParseResult<Expression> {
        loop {
            if self.peek_match(TokenType::OpenParen) {
                expression = self.parse_function_call(expression)?;
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
                TokenType::StringLiteral(value) => {
                    self.consume();
                    Expression::StringLiteral(value.clone())
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
                TokenType::Ref => {
                    self.consume();
                    let ident = self.parse_identifier()?;
                    Expression::Ref(ident)
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

    fn parse_function_call(&mut self, identifier: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::OpenParen)?;

        let mut arguments = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let expr = self.parse_expression()?;

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

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        match identifier {
            Expression::Identifier(identifier) => Ok(Expression::FunctionCall(FunctionCall {
                callee: Box::new(Expression::Identifier(identifier)),
                arguments,
            })),
            Expression::FieldAccess { object, field } => {
                Ok(Expression::FunctionCall(FunctionCall {
                    callee: Box::new(Expression::FieldAccess { object, field }),
                    arguments,
                }))
            }
            other => Ok(Expression::FunctionCall(FunctionCall {
                callee: Box::new(other),
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
        self.parse_binary_expression(0)
    }

    fn get_precedence(token_type: &TokenType) -> u8 {
        match token_type {
            TokenType::Assignment => 1,
            TokenType::Or => 2,
            TokenType::And => 3,
            TokenType::Equals | TokenType::NotEquals => 4,
            TokenType::GreaterThan
            | TokenType::GreaterThanOrEqual
            | TokenType::LessThan
            | TokenType::LessThanOrEqual => 5,
            TokenType::Plus | TokenType::Minus => 6,
            TokenType::Multiply | TokenType::Divide => 7,
            _ => 0,
        }
    }

    fn parse_binary_expression(&mut self, min_prec: u8) -> ParseResult<Expression> {
        let mut left = self.parse_unary_expression()?;

        while let Some(token) = self.peek() {
            let prec = Self::get_precedence(&token.token_type);
            if prec < min_prec || prec == 0 {
                break;
            }
            let operator = self.consume().unwrap().clone();

            let next_min_prec = prec + 1;
            let right = self.parse_binary_expression(next_min_prec)?;

            left = Expression::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
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
                TokenType::Ref => {
                    // Receiver shortcut: `ref self` == `self: ref Self`
                    self.consume();
                    let name = self.parse_identifier()?;
                    if name.name != "self" {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", TokenType::Identifier(name.name)),
                            pos: Some(SourcePos { line: name.line, col: name.col }),
                        });
                    }

                    let arg_type = Type::Reference(Box::new(Type::Identifier(Identifier {
                        name: "Self".to_string(),
                        line: name.line,
                        col: name.col,
                    })));

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Identifier(ref ident) if ident == "self" => {
                    // `self` receiver; optional explicit type via `self: ...`.
                    let (line, col) = match self.consume() {
                        Some(t) => (t.line, t.col),
                        None => return Err(ParseError::UnexpectedEof { pos: self.peek_previous().map(|t| SourcePos { line: t.line, col: t.col }) }),
                    };

                    let name = Identifier { name: "self".to_string(), line, col };

                    let arg_type = if self.consume_match(TokenType::Colon).is_ok() {
                        self.parse_type()?
                    } else {
                        Type::Identifier(Identifier { name: "Self".to_string(), line, col })
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

    fn parse_attributes(&mut self) -> ParseResult<Vec<Attribute>> {
        let mut attributes = Vec::new();

        while self.peek_match(TokenType::At) {
            attributes.push(self.parse_attribute()?);
        }

        Ok(attributes)
    }

    fn parse_enum_declaration(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
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

        Ok(Statement::EnumDeclaration {
            name,
            attributes,
            values,
        })
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
        let function = self.parse_function_declaration(vec![])?;

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

    fn parse_struct_declaration(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
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
                return Ok(Statement::StructDeclaration { name, attributes, fields, methods: vec![] });
            },
            TokenType::OpenBrace => {
                let methods = self.parse_struct_method_list()?;
                return Ok(Statement::StructDeclaration { name, attributes, fields, methods });
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

    fn parse_proto_declaration(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
        self.consume_match(TokenType::Proto)?;
        let name = self.parse_identifier()?;

        let methods = if self.peek_match(TokenType::OpenBrace) {
            self.parse_proto_method_list()?
        } else {
            self.consume_match(TokenType::Semicolon)?;
            vec![]
        };

        Ok(Statement::ProtoDeclaration {
            name,
            attributes,
            methods,
        })
    }

    fn parse_function_declaration(
        &mut self,
        attributes: Vec<Attribute>,
    ) -> ParseResult<FunctionDeclaration> {
        self.consume_match(TokenType::Fn)?;

        let name = self.parse_identifier()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Parse effect qualifiers (throws) after return type or parameters
        let mut effects = vec![];
        if self.peek_match(TokenType::Throws) {
            if let Some(token) = self.consume() {
                effects.push(Identifier {
                    name: "throws".to_string(),
                    line: token.line,
                    col: token.col,
                });
            }
        }

        let body = self.parse_block()?;

        Ok(FunctionDeclaration {
            name,
            attributes,
            effects,
            parameters,
            body,
            return_type,
        })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        if let Some(token) = self.peek() {
            match &token.token_type {
                TokenType::Ref => {
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
                    if module_path.is_empty() || module_path.ends_with('.') {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                            pos: self.peek().map(|t| SourcePos {
                                line: t.line,
                                col: t.col,
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
            module_path,
            alias,
        })
    }

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        // First, try to parse attributes
        let attributes = self.parse_attributes()?;

        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Import) => {
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on import statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                        }),
                    });
                }
                self.parse_import_statement()
            }
            Some(TokenType::Fn) => self
                .parse_function_declaration(attributes)
                .map(Statement::FunctionDeclaration),
            Some(TokenType::Enum) => self.parse_enum_declaration(attributes),
            Some(TokenType::Struct) => self.parse_struct_declaration(attributes),
            Some(TokenType::Proto) => self.parse_proto_declaration(attributes),
            // Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => {
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on return statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                        }),
                    });
                }
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Return(Box::new(expr)))
            }
            Some(TokenType::Throw) => {
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on throw statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
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
                        }),
                    });
                }
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
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on expression statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
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

    pub fn parse(&mut self) -> ParseResult<Vec<Statement>> {
        let mut statements = Vec::new();

        while self.peek().is_some() {
            statements.push(self.parse_statement()?);
        }

        Ok(statements)
    }
}
