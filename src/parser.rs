use std::{error::Error, fmt::Display};

use crate::lexer::{Token, TokenType};

#[derive(Debug, PartialEq, Clone)]
pub struct Identifier {
    pub name: String,
}
#[derive(Debug, PartialEq, Clone)]
pub struct Argument {
    pub name: Identifier,
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

const BINARY_OPTERATORS: &[TokenType] = &[
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

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        identifier: Identifier,
        expression: Expression,
    },
    Expression(Expression),
    FunctionDeclaration {
        name: Identifier,
        effects: Vec<Identifier>,
        parameters: Vec<Argument>,
        return_type: Option<Identifier>,
        body: Expression,
    },
    TryElse {
        try_block: Expression,
        else_block: Option<Expression>,
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
}

impl Display for ParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for ParserError {}

type ParseResult<T> = Result<T, ParserError>;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

macro_rules! match_token {
    ($token:expr, $($pattern:pat => $body: expr $(,)?)*) => {
        match $token {
            $(Some(&Token {
                token_type: $pattern,
                ..
            }) => $body,)*
            Some(t) => Err(ParserError::UnexpectedToken(t.clone())),
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
        match self.peek() {
            Some(t) if t.token_type == token_type => Ok(self.consume().unwrap()),
            Some(t) => Err(ParserError::ExpectedToken(token_type, t.clone())),
            _ => Err(ParserError::UnexpectedEof),
        }
    }

    fn parse_identifier(&mut self) -> ParseResult<Identifier> {
        match self.consume() {
            Some(Token {
                token_type: TokenType::Identifier(literal),
                ..
            }) => Ok(Identifier {
                name: literal.clone(),
            }),
            Some(t) => Err(ParserError::ExpectedToken(
                TokenType::Identifier("".to_string()),
                t.clone(),
            )),
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
        self.consume_match(TokenType::OpenParen)?;
        let mut arguments = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            arguments.push(self.parse_expression()?);
            let _ = self.consume_match(TokenType::Comma);
        }

        Ok(Expression::FunctionCall(FunctionCall {
            name: identifier.name,
            arguments,
        }))
    }

    fn parse_field_access(&mut self, object: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::Dot)?;
        let field = self.parse_identifier()?;

        if self.peek_match(TokenType::OpenParen) {
            let call = self.parse_function_call(field)?;
            return Ok(Expression::FieldAccess {
                object: Box::new(object),
                field: Identifier {
                    name: (call.clone()).get_name(),
                },
            });
        }

        Ok(Expression::FieldAccess {
            object: Box::new(object),
            field,
        })
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expression> {
        match_token! {
            self.consume(),
            TokenType::Integer(value) => {
                Ok(Expression::Number(value))
            }
            TokenType::Identifier(ref literal) => {
                let identifier = Identifier {
                    name: literal.clone(),
                };

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
            Expression::Identifier(identifier) => Ok(Expression::FunctionCall(FunctionCall {
                name: identifier.name,
                arguments,
            })),
            Expression::FieldAccess { object, field } => {
                Ok(Expression::FunctionCall(FunctionCall {
                    name: format!("{}.{}", object.clone().get_name(), field.name),
                    arguments,
                }))
            }
            _ => Ok(Expression::FunctionCall(FunctionCall {
                name: identifier.get_name(),
                arguments,
            })),
        }
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_primary_expression()?;
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
        self.consume_match(TokenType::If)?;
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
        })
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
        self.consume_match(TokenType::OpenParen)?;
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let name = self.parse_identifier()?;
            self.consume_match(TokenType::Colon)?;

            let arg_type = self.parse_identifier()?;

            parameters.push(Argument { name, arg_type });

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

            if !self.peek_match(TokenType::CloseBrace) {
                self.consume_match(TokenType::Comma)?;
            }
        }

        Ok(Statement::EnumDeclaration { name, values })
    }

    fn parse_function_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Fn)?;
        let mut effects = vec![];
        if self.consume_match(TokenType::OpenBracket).is_ok() {
            loop {
                effects.push(self.parse_identifier()?);

                if !self.peek_match(TokenType::CloseBracket) {
                    self.consume_match(TokenType::Comma)?;
                } else {
                    let _ = self.consume_match(TokenType::Comma);
                    self.consume_match(TokenType::CloseBracket)?;

                    break;
                }
            }
        }

        let name = self.parse_identifier()?;
        let parameters = self.parse_argument_list()?;
        let mut return_type: Option<Identifier> = None;

        if self.consume_match(TokenType::RightArrow).is_ok() {
            return_type = Some(self.parse_identifier()?);
        }

        let body = self.parse_block()?;

        Ok(Statement::FunctionDeclaration {
            name,
            effects,
            parameters,
            body,
            return_type,
        })
    }

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Fn) => self.parse_function_declaration(),
            Some(TokenType::Enum) => self.parse_enum_declaration(),
            Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => {
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Expression(Expression::Return(Box::new(expr))))
            }
            Some(TokenType::Try) => {
                self.consume();
                let try_block = self.parse_expression()?;
                let else_block = if self.consume_match(TokenType::Else).is_ok() {
                    Some(self.parse_expression()?)
                } else {
                    None
                };

                self.consume_match(TokenType::Semicolon)?;

                Ok(Statement::TryElse {
                    try_block,
                    else_block,
                })
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
                match_token! {
                    self.peek(),
                    TokenType::Semicolon => {
                        self.consume();
                        Ok(Statement::Expression(expression))
                    }
                    TokenType::CloseBrace => Ok(Statement::Expression(
                        Expression::BlockValue(Box::new(expression)),
                    )),
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
