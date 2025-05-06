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
    UnexpectedEof,
}

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

    fn consume_match(&mut self, token_type: TokenType) -> bool {
        if self.peek_match(token_type) {
            self.consume();
            true
        } else {
            false
        }
    }

    fn consume(&mut self) -> Option<Token> {
        if self.position < self.tokens.len() {
            let token = self.tokens[self.position].clone();
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    fn parse_function_call(&mut self, identifier: Identifier) -> Option<Expression> {
        if let Some(token) = self.consume() {
            if token.token_type != TokenType::OpenParen {
                return None;
            }
            let mut arguments = Vec::new();
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseParen {
                    self.consume();
                    break;
                }
                if let Some(expression) = self.parse_expression() {
                    arguments.push(expression);
                } else {
                    return None;
                }
                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }
            return Some(Expression::FunctionCall(FunctionCall {
                name: identifier.name,
                arguments,
            }));
        }
        None
    }

    fn parse_field_access(&mut self, object: Expression) -> Option<Expression> {
        if self.peek_match(TokenType::Dot) {
            self.consume()?;
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
                    return Some(Expression::FieldAccess {
                        object: Box::new(object),
                        field: Identifier {
                            name: (call.clone()).get_name(),
                        },
                    });
                }
                return Some(Expression::FieldAccess {
                    object: Box::new(object),
                    field: field_identifier,
                });
            }
        }
        None
    }

    fn parse_primary_expression(&mut self) -> Option<Expression> {
        if let Some(token) = self.peek() {
            match token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Some(Expression::Number(value))
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
                    return Some(current);
                }
                TokenType::OpenBrace => {
                    self.consume();
                    let mut statements = Vec::new();
                    while let Some(token) = self.peek() {
                        if token.token_type == TokenType::CloseBrace {
                            self.consume();
                            break;
                        }
                        if let Some(statement) = self.parse_statement() {
                            statements.push(statement);
                        } else {
                            return None;
                        }
                    }

                    return Some(Expression::Block { statements });
                }
                _ => return None,
            }
        } else {
            None
        }
    }

    fn parse_function_call_with_expression(
        &mut self,
        identifier: Expression,
    ) -> Option<Expression> {
        if let Some(token) = self.consume() {
            if token.token_type != TokenType::OpenParen {
                return None;
            }
            let mut arguments = Vec::new();
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseParen {
                    self.consume();
                    break;
                }
                if let Some(expression) = self.parse_expression() {
                    arguments.push(expression);
                } else {
                    return None;
                }

                self.consume_match(TokenType::Comma);
            }
            match identifier.clone() {
                Expression::Identifier(identifier) => {
                    return Some(Expression::FunctionCall(FunctionCall {
                        name: identifier.name,
                        arguments,
                    }))
                }
                Expression::FieldAccess { object, field } => {
                    return Some(Expression::FunctionCall(FunctionCall {
                        name: format!("{}.{}", object.clone().get_name(), field.name),
                        arguments,
                    }))
                }
                _ => {
                    return Some(Expression::FunctionCall(FunctionCall {
                        name: identifier.get_name(),
                        arguments,
                    }))
                }
            }
        }
        None
    }

    fn parse_expression(&mut self) -> Option<Expression> {
        let mut left = self.parse_primary_expression()?;
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::Plus || token.token_type == TokenType::Minus {
                let operator = self.consume().unwrap();
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
        Some(left)
    }

    fn parse_if_expression(&mut self) -> Option<Expression> {
        self.consume()?;
        if let Some(condition) = self.parse_expression() {
            if let Some(then_branch) = self.parse_expression() {
                let else_branch = if self.consume_match(TokenType::Else) {
                    self.parse_expression().map(Box::new)
                } else {
                    None
                };
                return Some(Expression::If {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch,
                });
            }
        }
        None
    }

    fn parse_block(&mut self) -> Option<Expression> {
        if self.consume_match(TokenType::OpenBrace) {
            let mut statements = Vec::new();
            while let Some(token) = self.peek() {
                if token.token_type == TokenType::CloseBrace {
                    self.consume();
                    break;
                }
                if let Some(statement) = self.parse_statement() {
                    statements.push(statement)
                } else {
                    return None;
                }
            }

            return Some(Expression::Block { statements });
        } else {
            None
        }
    }

    fn parse_argument_list(&mut self) -> Vec<Argument> {
        let mut parameters = Vec::new();
        if let Some(token) = self.consume() {
            if token.token_type != TokenType::OpenParen {
                return Vec::new();
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
                    let identifier = Identifier { name: literal_name };
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
                            arg_type = Identifier { name: literal_type };
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
        }

        parameters
    }

    fn parse_enum_declaration(&mut self) -> Option<Statement> {
        self.consume()?;
        if let Some(Token {
            token_type: TokenType::Identifier(literal),
            ..
        }) = self.consume()
        {
            let name = Identifier {
                name: literal.clone(),
            };
            if !self.consume_match(TokenType::OpenBrace) {
                return None;
            }
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
                    return None;
                }
                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }
            return Some(Statement::EnumDeclaration { name, values });
        }
        None
    }

    fn parse_function_declaration(&mut self) -> Option<Statement> {
        self.consume()?;
        if let Some(Token {
            token_type: TokenType::Identifier(literal),
            ..
        }) = self.consume()
        {
            let name = Identifier {
                name: literal.clone(),
            };
            let parameters = self.parse_argument_list();
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
            if let Some(body) = self.parse_block() {
                return Some(Statement::FunctionDeclaration {
                    name,
                    parameters,
                    body,
                    return_type,
                });
            }
        }
        None
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Fn) => self.parse_function_declaration(),
            Some(TokenType::Enum) => self.parse_enum_declaration(),
            Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => self
                .consume()
                .and_then(|_| self.parse_expression())
                .map(|e| Statement::Expression(Expression::Return(Box::new(e)))),
            Some(TokenType::Try) => {
                self.consume()?;
                if let Some(try_block) = self.parse_expression() {
                    if self.peek_match(TokenType::Else) {
                        self.consume();
                        if let Some(else_block) = self.parse_expression() {
                            return Some(Statement::TryElse {
                                try_block,
                                else_block,
                            });
                        }
                    }
                }
                None
            }
            Some(TokenType::Throw) => {
                self.consume()?;
                if let Some(expression) = self.parse_expression() {
                    return Some(Statement::Throw(expression));
                }
                None
            }
            Some(TokenType::Let) => {
                self.consume();
                if let Some(Token {
                    token_type: TokenType::Identifier(literal),
                    ..
                }) = self.consume()
                {
                    if self.consume_match(TokenType::Assignment) {
                        if let Some(expression) = self.parse_expression() {
                            if self.consume_match(TokenType::Semicolon) {
                                return Some(Statement::Let {
                                    identifier: Identifier { name: literal },
                                    expression,
                                });
                            }
                        }
                    }
                }
                None
            }
            _ => {
                if let Some(expression) = self.parse_expression() {
                    match self.peek().map(|t| t.token_type.clone()) {
                        Some(TokenType::Semicolon) => {
                            self.consume();
                            return Some(Statement::Expression(expression));
                        }
                        Some(TokenType::CloseBrace) => {
                            return Some(Statement::Expression(Expression::BlockValue(Box::new(
                                expression,
                            ))));
                        }
                        _ => {}
                    }
                }
                None
            }
        }
    }

    pub fn parse(&mut self) -> Vec<Statement> {
        let mut statements = Vec::new();

        while self.peek().is_some() {
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            } else {
                break;
            }
        }
        statements
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
