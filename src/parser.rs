use crate::lexer::{Token, TokenType};

#[derive(Debug, PartialEq, Clone)]
pub struct Identifier {
    pub name: String,
}
#[derive(Debug, PartialEq, Clone)]
pub struct Argument {
    pub identifier: Identifier,
    pub arg_type: Identifier,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct EnumValue {
    pub name: String,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Identifier(Identifier),
    Number(i64),
    Binary {
        left: Box<Expression>,
        operator: Token,
        right: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        then_branch: Box<Expression>,
        else_branch: Option<Box<Expression>>,
    },
    FunctionCall(FunctionCall),
    Return(Box<Expression>),
    FieldAccess {
        object: Box<Expression>,
        field: Identifier,
    },
    Block {
        statements: Vec<Statement>,
    },
    BlockValue(Box<Expression>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        identifier: Identifier,
        expression: Expression,
    },
    Expression(Expression),
    FunctionDeclaration {
        name: Identifier,
        parameters: Vec<Argument>,
        return_type: Option<Identifier>,
        body: Expression,
    },
    TryElse {
        try_block: Expression,
        else_block: Expression,
    },
    Throw(Expression),
    EnumDeclaration {
        name: Identifier,
        values: Vec<EnumValue>,
    },
}

#[derive(Debug, PartialEq)]
pub enum ParserError {
    UnexpectedToken(Token),
    ExpectedToken(TokenType, Token),
    UnexpectedEof,
    InvalidExpression,
    InvalidStatement,
}

type ParseResult<T> = Result<T, ParserError>;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

macro_rules! peek_match {
    ($self:ident, $($pattern:pat => $body: expr $(,)?)*) => {
        match $self.peek() {
            Some(&Token {
                token_type: $pattern,
                ..
            }) => $body,
            Some(t) => Err(ParserError::UnexpectedToken(t)),
            _ => Err(ParserError::UnexpectedEof),
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

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn peek_match(&self, token_type: TokenType) -> bool {
        match self.peek() {
            Some(t) => t.token_type == token_type,
            _ => false,
        }
    }

    fn consume_match(&mut self, token_type: TokenType) -> ParseResult<&Token> {
        match self.consume() {
            Some(t) if t.token_type == token_type => Ok(t),
            Some(t) => Err(ParserError::ExpectedToken(token_type, t.clone())),
            _ => Err(ParserError::UnexpectedEof),
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
        if let Some(token) = self.consume() {
            if token.token_type != TokenType::OpenParen {
                return Err(ParserError::ExpectedToken(
                    TokenType::OpenParen,
                    token.clone(),
                ));
            }
            let mut arguments = Vec::new();
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseParen {
                    self.consume();
                    break;
                }

                arguments.push(self.parse_expression()?);

                let _ = self.consume_match(TokenType::Comma);
            }
            return Ok(Expression::FunctionCall(FunctionCall {
                name: identifier.name,
                arguments,
            }));
        } else {
            Err(ParserError::UnexpectedEof)
        }
    }

    fn parse_field_access(&mut self, object: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::Dot)?;
        if let Some(&Token {
            token_type: TokenType::Identifier(ref field_name),
            ..
        }) = self.peek()
        {
            let field_identifier = Identifier {
                name: field_name.clone(),
            };
            if self.peek_match(TokenType::OpenParen) {
                let call = self.parse_function_call(field_identifier)?;
                return Ok(Expression::FieldAccess {
                    object: Box::new(object),
                    field: Identifier {
                        name: (call.clone()).get_name(),
                    },
                });
            }
            return Ok(Expression::FieldAccess {
                object: Box::new(object),
                field: field_identifier,
            });
        } else {
            return Err(ParserError::InvalidExpression);
        }
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expression> {
        if let Some(token) = self.peek() {
            match token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Ok(Expression::Number(value))
                }
                TokenType::Identifier(ref literal) => {
                    let identifier = Identifier {
                        name: literal.clone(),
                    };
                    self.consume();
                    let mut current: Expression = Expression::Identifier(identifier);
                    loop {
                        if self.peek_match(TokenType::OpenParen) {
                            current = self.parse_function_call_with_expression(current)?;
                        } else if self.peek_match(TokenType::Dot) {
                            current = self.parse_field_access(current)?;
                        } else {
                            break;
                        }
                    }
                    Ok(current)
                }
                TokenType::OpenBrace => {
                    self.consume();
                    let mut statements = Vec::new();
                    while let Some(token) = self.peek() {
                        if token.token_type == TokenType::CloseBrace {
                            self.consume();
                            break;
                        }

                        statements.push(self.parse_statement()?);
                    }

                    Ok(Expression::Block { statements })
                }
                _ => Err(ParserError::UnexpectedToken(token.clone())),
            }
        } else {
            Err(ParserError::UnexpectedEof)
        }
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

            arguments.push(self.parse_expression()?);

            if !self.peek_match(TokenType::CloseParen) {
                self.consume_match(TokenType::Comma)?;
            }
        }
        match identifier.clone() {
            Expression::Identifier(identifier) => {
                return Ok(Expression::FunctionCall(FunctionCall {
                    name: identifier.name,
                    arguments,
                }))
            }
            Expression::FieldAccess { object, field } => {
                return Ok(Expression::FunctionCall(FunctionCall {
                    name: format!("{}.{}", object.clone().get_name(), field.name),
                    arguments,
                }))
            }
            _ => {
                return Ok(Expression::FunctionCall(FunctionCall {
                    name: identifier.get_name(),
                    arguments,
                }))
            }
        }
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_primary_expression()?;
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::Plus || token.token_type == TokenType::Minus {
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
        self.consume();
        if let Ok(condition) = self.parse_expression() {
            if let Ok(then_branch) = self.parse_expression() {
                let else_branch = if self.consume_match(TokenType::Else).is_ok() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                return Ok(Expression::If {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch,
                });
            }
        }
        Err(ParserError::InvalidExpression)
    }

    fn parse_block(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut statements = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            statements.push(self.parse_statement()?);
        }

        return Ok(Expression::Block { statements });
    }

    fn parse_argument_list(&mut self) -> ParseResult<Vec<Argument>> {
        let mut parameters = Vec::new();
        if let Some(token) = self.consume() {
            if token.token_type != TokenType::OpenParen {
                return Err(ParserError::ExpectedToken(
                    TokenType::OpenParen,
                    token.clone(),
                ));
            }
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseParen {
                    self.consume();
                    break;
                }

                if let Some(Token {
                    token_type: TokenType::Identifier(literal_name),
                    ..
                }) = self.consume()
                {
                    let identifier = Identifier {
                        name: literal_name.clone(),
                    };
                    let mut arg_type = Identifier {
                        name: "".to_string(),
                    };

                    if self.peek_match(TokenType::Colon) {
                        self.consume();
                        if let Some(Token {
                            token_type: TokenType::Identifier(literal_type),
                            ..
                        }) = self.consume()
                        {
                            arg_type = Identifier {
                                name: literal_type.clone(),
                            };
                        }
                    }

                    parameters.push(Argument {
                        identifier,
                        arg_type,
                    });
                }

                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }
            return Ok(parameters);
        } else {
            return Err(ParserError::UnexpectedEof);
        }
    }

    fn parse_enum_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Enum)?;
        let token = self.consume();

        if let Some(Token {
            token_type: TokenType::Identifier(literal),
            ..
        }) = token
        {
            let name = Identifier {
                name: literal.clone(),
            };

            self.consume_match(TokenType::OpenBrace)?;

            let mut values = Vec::new();
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseBrace {
                    self.consume();
                    break;
                }
                if let Some(Token {
                    token_type: TokenType::Identifier(literal),
                    ..
                }) = self.consume()
                {
                    values.push(EnumValue {
                        name: literal.clone(),
                    });
                } else {
                    return Err(ParserError::InvalidStatement);
                }
                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }

            return Ok(Statement::EnumDeclaration { name, values });
        } else if let Some(t) = token {
            return Err(ParserError::UnexpectedToken(t.clone()));
        } else {
            return Err(ParserError::UnexpectedEof);
        }
    }

    fn parse_function_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Fn)?;

        if let Some(Token {
            token_type: TokenType::Identifier(literal),
            ..
        }) = self.consume()
        {
            let name = Identifier {
                name: literal.clone(),
            };
            let parameters = self.parse_argument_list()?;
            let mut return_type: Option<Identifier> = None;
            if self.peek_match(TokenType::RightArrow) {
                self.consume();
                if let Some(Token {
                    token_type: TokenType::Identifier(literal),
                    ..
                }) = self.consume()
                {
                    return_type = Some(Identifier {
                        name: literal.clone(),
                    });
                }
            }
            if let Ok(body) = self.parse_block() {
                return Ok(Statement::FunctionDeclaration {
                    name,
                    parameters,
                    body,
                    return_type,
                });
            }
        }
        Err(ParserError::InvalidStatement)
    }

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Fn) => self
                .parse_function_declaration()
                .map_err(|err| err)
                .and_then(|stmt| Ok(stmt)),
            Some(TokenType::Enum) => self
                .parse_enum_declaration()
                .map_err(|err| err)
                .and_then(|stmt| Ok(stmt)),
            Some(TokenType::If) => self
                .parse_if_expression()
                .map_err(|err| err)
                .map(Statement::Expression),
            Some(TokenType::Return) => {
                let _ = self.consume();
                Ok(Statement::Expression(Expression::Return(Box::new(self.parse_expression()?))))
            }
            Some(TokenType::Try) => {
                self.consume();
                if let Ok(try_block) = self.parse_expression() {
                    if self.peek_match(TokenType::Else) {
                        self.consume();
                        if let Ok(else_block) = self.parse_expression() {
                            return Ok(Statement::TryElse {
                                try_block,
                                else_block,
                            });
                        }
                    }
                }

                return Err(ParserError::InvalidStatement);
            }
            Some(TokenType::Throw) => {
                self.consume();
                return self
                    .parse_expression()
                    .map_err(|err| err)
                    .map(|expression| Statement::Throw(expression));
            }
            Some(TokenType::Let) => {
                self.consume();

                if let Some(Token {
                    token_type: TokenType::Identifier(literal),
                    ..
                }) = self.consume().cloned()
                {
                    self.consume_match(TokenType::Assignment)?;
                    let expression = self.parse_expression()?;

                    self.consume_match(TokenType::Semicolon)?;
                    Ok(Statement::Let {
                        identifier: Identifier {
                            name: literal.clone(),
                        },
                        expression,
                    })
                } else {
                    Err(ParserError::InvalidStatement)
                }
            }
            _ => {
                let expression = self.parse_expression()?;
                match self.peek().map(|t| t.token_type.clone()) {
                    Some(TokenType::Semicolon) => {
                        self.consume();
                        Ok(Statement::Expression(expression))
                    }
                    Some(TokenType::CloseBrace) => Ok(Statement::Expression(
                        Expression::BlockValue(Box::new(expression)),
                    )),
                    _ => Err(ParserError::InvalidStatement),
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

impl Expression {
    pub fn get_name(&self) -> String {
        match self {
            Expression::Identifier(identifier) => identifier.name.clone(),
            Expression::FunctionCall(function_call) => function_call.name.clone(),
            Expression::FieldAccess { object, field } => {
                format!("{}.{}", object.get_name(), field.name)
            }
            _ => "".to_string(),
        }
    }
}
