use crate::lexer::Token;

#[derive(Debug, PartialEq, Clone)]
pub struct Identifier {
    pub name: String,
}
#[derive(Debug, PartialEq, Clone)]
pub struct Argument {
    pub identifier: Identifier,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<Expression>,
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
        body: Expression,
    },
    TryElse {
        try_block: Expression,
        else_block: Expression,
    },
    Throw(Expression),
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
                    if self.peek() == Some(&Token::OpenParen) {
                        return self.parse_function_call(identifier);
                    } else if self.peek() == Some(&Token::Dot) {
                        self.consume();
                        if let Some(Token::Identifier(literal)) = self.consume() {
                            return Some(Expression::FieldAccess {
                                object: Box::new(Expression::Identifier(identifier)),
                                field: Identifier {
                                    name: literal.clone(),
                                },
                            });
                        }
                    } else {
                        Some(Expression::Identifier(identifier))
                    }
                }
                Token::OpenBrace => {
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
                    Some(Expression::Block { statements })
                }
                _ => None,
            }
        } else {
            None
        }
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
                Some(Expression::Block { statements })
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
                if let Some(Token::Identifier(literal)) = self.consume() {
                    parameters.push(Argument {
                        identifier: Identifier { name: literal },
                    });
                }
                if self.peek() == Some(&Token::Comma) {
                    self.consume();
                }
            }
        }
        parameters
    }
    fn parse_function_declaration(&mut self) -> Option<Statement> {
        self.consume()?;
        if let Some(Token::Identifier(ref literal)) = self.consume() {
            let name = Identifier {
                name: literal.clone(),
            };
            let parameters = self.parse_argument_list();
            if let Some(body) = self.parse_expression() {
                return Some(Statement::FunctionDeclaration {
                    name,
                    parameters,
                    body,
                });
            }
        }
        None
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        match self.peek() {
            Some(Token::Fn) => self.parse_function_declaration(),
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
                    if self.consume() == Some(Token::Semicolon) {
                        return Some(Statement::Expression(expression));
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
                return Vec::new();
            }
        }
        statements
    }
}
