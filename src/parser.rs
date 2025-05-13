use std::{error::Error, fmt::Display};

use crate::lexer::{Token, TokenType};

#[derive(Debug, PartialEq, Clone)]
pub struct Identifier {
    pub name: String,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Type {
    Identifier(Identifier),
    Reference(Box<Type>),
    Array(Box<Type>),
}

impl Type {
    pub fn get_name(&self) -> String {
        match self {
            Type::Identifier(identifier) => identifier.name.clone(),
            Type::Reference(r) => {
                format!("&{}", r.get_name())
            }
            Type::Array(a) => {
                format!("{}[]", a.get_name())
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Argument {
    pub name: Identifier,
    pub arg_type: Type,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionDeclaration {
    pub name: Identifier,
    pub effects: Vec<Identifier>,
    pub parameters: Vec<Argument>,
    pub return_type: Option<Type>,
    pub body: StatementBlock,
}

#[derive(Debug, PartialEq, Clone)]
pub struct StructField {
    pub is_pub: bool,
    pub name: Identifier,
    pub field_type: Type,
    pub default_value: Option<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct StructMethod {
    pub is_pub: bool,
    pub function: FunctionDeclaration,
}

#[derive(Debug, PartialEq, Clone)]
pub struct EnumValue {
    pub name: String,
}

type StatementBlock = Vec<Statement>;

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Identifier(Identifier),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    Unary {
        operator: Token,
        right: Box<Expression>,
    },
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
    StructInitiation(StructInitiation),
    Block(StatementBlock),
    Try {
        try_block: Box<Expression>,
        else_block: Option<Box<Expression>>,
    },

    BlockValue(Box<Expression>),
}

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

#[derive(Debug, PartialEq, Clone)]
pub struct StructInitiation {
    pub struct_type: Identifier,
    pub fields: Vec<(Identifier, Option<Expression>)>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Pattern {
    Identifier(Identifier),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    FieldAccess {
        object: Box<Pattern>,
        field: Identifier,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub struct NamedPattern {
    pub name: Option<Identifier>,
    pub pattern: Pattern,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        identifier: Identifier,
        expression: Expression,
    },
    Expression(Expression),
    FunctionDeclaration(FunctionDeclaration),
    Throw(Expression),
    EnumDeclaration {
        name: Identifier,
        values: Vec<EnumValue>,
    },
    StructDeclaration {
        name: Identifier,
        fields: Vec<StructField>,
    },
    Branch {
        named_pattern: NamedPattern,
        expression: Expression,
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
    ($token:expr, $($pattern:pat $(if $guard:expr)? => $body: expr $(,)?)*) => {
        match $token {
            $(Some(&Token {
                token_type: $pattern,
                ..
            }) $(if $guard)? => $body,)*
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
            Some(t) => t.token_type == token_type,
            _ => false,
        }
    }

    fn consume_match(&mut self, token_type: TokenType) -> ParseResult<()> {
        match self.peek() {
            Some(t) if t.token_type == token_type => {
                self.consume().unwrap();
                Ok(())
            }
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

    fn parse_struct_initiation(&mut self, struct_type: Identifier) -> ParseResult<Expression> {
        self.consume_match(TokenType::OpenBrace)?;

        let mut fields = Vec::new();
        while !self.peek_match(TokenType::CloseBrace) {
            let field_name = self.parse_identifier()?;
            let field_value = if self.consume_match(TokenType::Colon).is_ok() {
                Some(self.parse_expression()?)
            } else {
                None
            };

            fields.push((field_name, field_value));

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseBrace) {
                comma?;
            }
        }

        self.consume_match(TokenType::CloseBrace)?;

        Ok(Expression::StructInitiation(StructInitiation {
            struct_type,
            fields,
        }))
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
        let expression = match_token! {
            self.consume(),
            TokenType::Integer(value) => Ok(Expression::IntegerLiteral(value)),
            TokenType::Float(value) => Ok(Expression::FloatLiteral(value)),
            TokenType::Identifier(ref literal) => {
                let identifier = Identifier { name: literal.clone() };
                if self.peek_match(TokenType::OpenBrace) {
                    self.parse_struct_initiation(identifier)
                } else {
                    Ok(Expression::Identifier(identifier))
                }
            },
            TokenType::OpenBrace => {
                let mut statements = Vec::new();
                while let Some(token) = self.peek() {
                    if token.token_type == TokenType::CloseBrace {
                        self.consume();
                        break;
                    }

                    statements.push(self.parse_statement()?)
                }
                Ok(Expression::Block(statements))
            },
            TokenType::Try => {
                self.unconsume();
                self.parse_try_expression()
            },
            TokenType::If => {
                self.unconsume();
                self.parse_if_expression()
            },
        }?;

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

            arguments.push(self.parse_expression()?);

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
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
        let pattern = match_token! {
            self.consume(),
            TokenType::Integer(value) => Ok(Pattern::IntegerLiteral(value)),
            TokenType::Float(value) => Ok(Pattern::FloatLiteral(value)),
            TokenType::StringLiteral(ref value) => Ok(Pattern::StringLiteral(value.clone())),
            TokenType::Identifier(ref literal) => {
                let identifier = Pattern::Identifier(Identifier { name: literal.clone() });
                let identifier = self.parse_pattern_suffix(identifier)?;

                Ok(identifier)
            },
        };

        pattern
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

    fn parse_argument_list(&mut self) -> ParseResult<Vec<Argument>> {
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
                    self.consume();
                    self.consume_match(TokenType::Identifier("self".to_string()))?;
                    Ok(Argument {
                        name: Identifier {name: "self".to_string()},
                        arg_type: Type::Reference(Box::new(Type::Identifier(Identifier {name: "Self".to_string()})))
                     })
                },
                TokenType::Identifier(ref ident) if ident == "self" => {
                    self.consume();
                    Ok(Argument {
                        name: Identifier {name: "self".to_string()},
                        arg_type: Type::Identifier(Identifier {name: "Self".to_string()})
                    })
                },
                TokenType::Identifier(_) => {
                    let name = self.parse_identifier()?;
                    self.consume_match(TokenType::Colon)?;

                    let arg_type = self.parse_type()?;
                    Ok(Argument { name, arg_type })
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

    fn parse_struct_field(&mut self, is_pub: bool) -> ParseResult<StructField> {
        let field_name = self.parse_identifier()?;
        self.consume_match(TokenType::Colon)?;
        let field_type = self.parse_type()?;
        let default_value = if self.consume_match(TokenType::Assignment).is_ok() {
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.consume_match(TokenType::Semicolon)?;

        Ok(StructField {
            is_pub,
            name: field_name,
            field_type,
            default_value,
        })
    }

    fn parse_struct_method(&mut self, is_pub: bool) -> ParseResult<StructMethod> {
        let function = self.parse_function_declaration()?;

        Ok(StructMethod { is_pub, function })
    }

    fn parse_struct_declaration(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Struct)?;
        let name = self.parse_identifier()?;
        self.consume_match(TokenType::OpenBrace)?;

        let mut fields = vec![];
        let mut methods = vec![];
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            let is_pub = self.consume_match(TokenType::Pub).is_ok();
            let res = match_token! {
                self.peek(),
                TokenType::Fn => {
                    methods.push(self.parse_struct_method(is_pub)?);
                    Ok(())
                },
                TokenType::Identifier(_) => {
                    fields.push(self.parse_struct_field(is_pub)?);
                    Ok(())
                },
            };

            res?;
        }

        Ok(Statement::StructDeclaration { name, fields })
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
        match_token! {
            self.consume(),
            TokenType::Ampersand => {
                let ty = self.parse_type()?;
                Ok(Type::Reference(Box::new(ty)))
            },
            TokenType::OpenBracket => {
                let ty = self.parse_type()?;
                self.consume_match(TokenType::CloseBracket)?;
                Ok(Type::Array(Box::new(ty)))
            },
            TokenType::Identifier(ref identifier) => {
                Ok(Type::Identifier(Identifier { name: identifier.clone() }))
            },
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
                Ok(Statement::Expression(Expression::Return(Box::new(expr))))
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
