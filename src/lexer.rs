use std::fmt;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    // Keywords
    Enum,
    Fn,
    Struct,
    Let,
    Pub,
    Return,
    If,
    Throw,
    Try,
    Else,

    // Identifiers and Literals
    Identifier(String),
    Integer(i64),
    Float(f64),
    StringLiteral(String),

    // Operators
    Plus,
    Minus,
    Equals,
    NotEquals,
    Multiply,
    Divide,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    And,
    Or,
    Ampersand,
    Not,
    Assignment,

    // Punctuation
    Colon,
    Comma,
    Dot,
    Semicolon,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    RightArrow,
    FatRightArrow,

    // End of File
    EOF,
}

#[derive(Debug, PartialEq)]
pub enum LexerError{
    UnexpectedCharacter(char, (usize, usize)),
    UnterminatedString((usize, usize)),
    InvalidNumber(String, (usize, usize)),
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LexerError::UnexpectedCharacter(c, (line, col)) => {
                write!(f, "Unexpected character: {} at line: {} column: {}", c, line, col)
            }
            LexerError::UnterminatedString((line, col)) => {
                write!(f, "Unterminated string at line: {} column: {}", line, col)
            }
            LexerError::InvalidNumber(num, (line, col)) => {
                write!(f, "Invalid number: {} at line: {} column: {}", num, line, col)
            }
        }

    }
}

#[derive(Debug)]
pub struct Lexer {
    input: String,
    position: usize,
    line: usize,
    col: usize,
    pub errors: Vec<LexerError>,
}

impl Lexer {
    pub fn new(input: String) -> Self {
        let lexer = Lexer {
            input,
            position: 0,
            line: 1,
            col: 1,
            errors: vec![],
        };

        lexer
    }

    fn read_char(&mut self) {
        if self.position < self.input.len() {
            self.col += 1;
            self.position += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input.chars().nth(self.position)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if !c.is_whitespace() {
                break;
            }
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                 self.col += 1;
            }
            
            self.read_char();
        }
    }

    fn read_identifier(&mut self) -> String {
        let start_position = self.position;
        while let Some(c) = self.peek_char() {
            if !c.is_alphanumeric() && c != '_' {
                break;
            }

            self.read_char();
        }

        self.input[start_position..self.position].to_string()
    }

    fn read_number(&mut self) -> String {
        let start_position = self.position;
        while let Some(c) = self.peek_char() {
            if !c.is_numeric() && c != '.' && c != '_' {
                break;
            }
            self.read_char();
        }
        self.input[start_position..self.position].to_string()
    }

    fn read_string(&mut self) -> Result<String, LexerError> {
        if self.peek_char() != Some('"') {
            return Err(LexerError::UnexpectedCharacter(
                self.peek_char().unwrap(),
                (self.line, self.col),
            ));
        }

        if self.position == self.input.len() - 1 {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        } 

        let start_position = self.position + 1;
        self.read_char();
        while let Some(c) = self.peek_char() {
            if c == '"' {
                break;
            }
            self.read_char();
        }
        if self.peek_char() != Some('"') {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        let result = self.input[start_position..self.position].to_string();
        self.read_char();
        Ok(result)
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        let token = match self.peek_char() {
            Some('+') => {
                self.read_char();
                Token::Plus
            }
            Some('-') => {
                self.read_char();
                if self.peek_char() == Some('>') {
                    self.read_char();
                    Token::RightArrow
                } else {
                    Token::Minus
                }
            }
            Some('*') => {
                self.read_char();
                Token::Multiply
            }
            Some('/') => {
                self.read_char();
                if self.peek_char() == Some('/') {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.read_char();
                    }
                    return self.next_token();
                } else {
                    Token::Divide
                }
            }
            Some('=') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    Token::Equals
                } else if self.peek_char() == Some('>') {
                    self.read_char();
                    Token::FatRightArrow
                } else {
                    Token::Assignment
                }
            }
            Some('!') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::NotEquals
                } else {
                    Token::Not
                }
            }
            Some('>') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::GreaterThanOrEqual
                } else {
                    Token::GreaterThan
                }
            }
            Some('<') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    Token::LessThanOrEqual
                } else {
                    Token::LessThan
                }
            }
            Some('&') => {
                self.read_char();
                Token::Ampersand
            }
            Some(',') => {
                self.read_char();
                Token::Comma
            }
            Some('.') => {
                self.read_char();
                Token::Dot
            }
            Some(';') => {
                self.read_char();
                Token::Semicolon
            }
            Some(':') => {
                self.read_char();
                Token::Colon
            }
            Some('{') => {
                self.read_char();
                Token::OpenBrace
            }
            Some('}') => {
                self.read_char();
                Token::CloseBrace
            }
            Some('(') => {
                self.read_char();
                Token::OpenParen
            }
            Some(')') => {
                self.read_char();
                Token::CloseParen
            }
            Some('[') => {
                self.read_char();
                Token::OpenBracket
            }
            Some(']') => {
                self.read_char();
                Token::CloseBracket
            }
            Some('"') => match self.read_string() {
                Ok(string) => Token::StringLiteral(string),
                Err(err) => {
                    self.errors.push(err);
                    Token::EOF
                }
            },

            Some(c) => {
                if c.is_alphabetic() || c == '_' {
                    let ident = self.read_identifier();
                    match ident.as_str() {
                        "enum" => Token::Enum,
                        "fn" => Token::Fn,
                        "struct" => Token::Struct,
                        "let" => Token::Let,
                        "pub" => Token::Pub,
                        "return" => Token::Return,
                        "if" => Token::If,
                        "throw" => Token::Throw,
                        "try" => Token::Try,
                        "else" => Token::Else,
                        _ => Token::Identifier(ident),
                    }
                } else if c.is_numeric() {
                    let current_position = self.position;
                    let num = self.read_number();

                    let parsed_number = num.parse::<f64>();

                    match parsed_number {
                        Ok(n) => {
                            if num.contains('.') {
                                Token::Float(n)
                            } else {
                                Token::Integer(n as i64)
                            }
                        }
                        Err(_) => {
                            self.errors
                                .push(LexerError::InvalidNumber(num, (self.line, self.col)));
                            Token::EOF
                        }
                    }
                } else {
                    panic!("Unexpected character: {}", c);
                }
            }
            None => Token::EOF,
        };

        token
    }
}
