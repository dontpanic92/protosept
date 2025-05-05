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

#[derive(Debug)]
pub struct Lexer {
    input: String,
    position: usize,
}

impl Lexer {
    pub fn new(input: String) -> Self {
        let lexer = Lexer {
            input,
            position: 0,
        };

        lexer
    }

    fn read_char(&mut self) {
        if self.position < self.input.len() {
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
            if !c.is_numeric() && c != '.' {
                break;
            }
            self.read_char();
        }
        self.input[start_position..self.position].to_string()
    }

    fn read_string(&mut self) -> String {
        let start_position = self.position + 1;
        self.read_char();
        while let Some(c) = self.peek_char() {
            if c == '"' {
                break;
            }
            self.read_char();
        }
        let result = self.input[start_position..self.position].to_string();
        self.read_char();

        result
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
            Some('"') => Token::StringLiteral(self.read_string()),

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
                    let num = self.read_number();
                    if num.contains(".") {
                        Token::Float(num.parse().unwrap())
                    } else {
                        Token::Integer(num.parse().unwrap())
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
