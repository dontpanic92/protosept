use crate::lexer::Token;

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

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
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
            if token != Token::OpenParen {
                return None;
            }
            let mut arguments = Vec::new();
            while let Some(token) = self.peek() {
                if token == &Token::CloseParen {
                    self.consume();
                    break;
                }
                if let Some(expression) = self.parse_expression() {
                    arguments.push(expression);
                } else {
                    return None;
                }
                if self.peek() == Some(&Token::Comma) {
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
        if let Some(Token::Dot) = self.consume() {
            if let Some(Token::Identifier(field_name)) = self.consume() {
                let field_identifier = Identifier { name: field_name };
                if self.peek() == Some(&Token::OpenParen) {
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
            match token {
                Token::Integer(value) => {
                    let value = *value;
                    self.consume();
                    Some(Expression::Number(value))
                }
                Token::Identifier(ref literal) => {
                    let identifier = Identifier {
                        name: literal.clone(),
                    };
                    self.consume();
                    let mut current: Expression = Expression::Identifier(identifier);
                    loop {
                        if self.peek() == Some(&Token::OpenParen) {
                            current = self.parse_function_call_with_expression(current)?;
                        } else if self.peek() == Some(&Token::Dot) {
                            current = self.parse_field_access(current)?;
                        } else {
                            break;
                        }
                    }
                    return Some(current);
                }
                Token::OpenBrace => {
                    self.consume();
                    let mut statements = Vec::new();
                    let mut last_expression = None;
                    while let Some(token) = self.peek() {
                        if token == &Token::CloseBrace {
                            self.consume();
                            break;
                        }
                        if self.peek() == Some(&Token::Semicolon) {
                            self.consume();
                        } else {
                            if let Some(statement) = self.parse_statement() {
                                statements.push(statement);
                            } else if let Some(expression) = self.parse_expression() {
                                last_expression = Some(expression);
                            } else {
                                return None;
                            }
                        }
                    }
                    if let Some(last_expression) = last_expression {
                        statements.push(Statement::Expression(last_expression));
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
            if token != Token::OpenParen {
                return None;
            }
            let mut arguments = Vec::new();
            while let Some(token) = self.peek() {
                if token == &Token::CloseParen {
                    self.consume();
                    break;
                }
                if let Some(expression) = self.parse_expression() {
                    arguments.push(expression);
                } else {
                    return None;
                }
                if self.peek() == Some(&Token::Comma) {
                    self.consume();
                }
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
            if token == &Token::Plus || token == &Token::Minus {
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
                let else_branch = if self.peek() == Some(&Token::Else) {
                    self.consume();
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
        if let Some(token) = self.peek() {
            if token == &Token::OpenBrace {
                self.consume();
                let mut statements = Vec::new();
                while let Some(token) = self.peek() {
                    if token == &Token::CloseBrace {
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
        } else {
            None
        }
    }

    fn parse_argument_list(&mut self) -> Vec<Argument> {
        let mut parameters = Vec::new();
        if let Some(token) = self.consume() {
            if token != Token::OpenParen {
                return Vec::new();
            }
            while let Some(token) = self.peek() {
                if token == &Token::CloseParen {
                    self.consume();
                    break;
                }

                if let Some(Token::Identifier(literal_name)) = self.consume() {
                    let identifier = Identifier { name: literal_name };
                    let mut arg_type = Identifier {
                        name: "".to_string(),
                    };

                    if self.peek() == Some(&Token::Colon) {
                        self.consume();
                        if let Some(Token::Identifier(literal_type)) = self.consume() {
                            arg_type = Identifier { name: literal_type };
                        }
                    }

                    parameters.push(Argument {
                        identifier,
                        arg_type,
                    });
                }

                if self.peek() == Some(&Token::Comma) {
                    self.consume();
                }
            }
        }
        
        parameters
    }

    fn parse_enum_declaration(&mut self) -> Option<Statement> {
        self.consume()?;
        if let Some(Token::Identifier(ref literal)) = self.consume() {
            let name = Identifier {
                name: literal.clone(),
            };
            if self.consume() != Some(Token::OpenBrace) {
                return None;
            }
            let mut values = Vec::new();
            while let Some(token) = self.peek() {
                if token == &Token::CloseBrace {
                    self.consume();
                    break;
                }
                if let Some(Token::Identifier(literal)) = self.consume() {
                    values.push(EnumValue {
                        name: literal.clone(),
                    });
                } else {
                    return None;
                }
                if self.peek() == Some(&Token::Comma) {
                    self.consume();
                }
            }
            return Some(Statement::EnumDeclaration { name, values });
        }
        None
    }

    fn parse_function_declaration(&mut self) -> Option<Statement> {
        self.consume()?;
        if let Some(Token::Identifier(ref literal)) = self.consume() {
            let name = Identifier {
                name: literal.clone(),
            };
            let parameters = self.parse_argument_list();
            let mut return_type: Option<Identifier> = None;
            if self.peek() == Some(&Token::RightArrow) {
                self.consume();
                if let Some(Token::Identifier(ref literal)) = self.consume() {
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
        match self.peek() {
            Some(Token::Fn) => self.parse_function_declaration(),
            Some(Token::Enum) => self.parse_enum_declaration(),
            Some(Token::If) => self.parse_if_expression().map(Statement::Expression),
            Some(Token::Return) => self
                .consume()
                .and_then(|_| self.parse_expression())
                .map(|e| Statement::Expression(Expression::Return(Box::new(e)))),
            Some(Token::Try) => {
                self.consume()?;
                if let Some(try_block) = self.parse_expression() {
                    if self.peek() == Some(&Token::Else) {
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
            Some(Token::Throw) => {
                self.consume()?;
                if let Some(expression) = self.parse_expression() {
                    return Some(Statement::Throw(expression));
                }
                None
            }
            Some(Token::Let) => {
                self.consume();
                if let Some(Token::Identifier(literal)) = self.consume() {
                    if self.consume() == Some(Token::Assignment) {
                        if let Some(expression) = self.parse_expression() {
                            if self.consume() == Some(Token::Semicolon) {
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
                    match self.peek() {
                        Some(Token::Semicolon) => {
                            self.consume();
                            return Some(Statement::Expression(expression));
                        }
                        Some(Token::CloseBrace) => {
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
