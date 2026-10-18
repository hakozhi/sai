use crate::{diagnostics::Diagnostic, parser::keyword::is_keyword};

use super::position::Position;

#[derive(Debug, Clone)]
pub struct Lexer {
    input: Vec<char>,
    index: usize,
    position: Position,
    current_char: Option<char>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: TokenType,
    pub value: String,
    pub position: Position,
}

#[derive(Debug, Clone)]
pub enum TokenType {
    Keyword,
    Symbol,
    StringLiteral,
    IntLiteral,
    FloatLiteral,
    BoolLiteral,
    LParen,
    RParen,
}

impl Lexer {
    pub fn new(filename: String, source_code: &str) -> Self {
        let chars: Vec<char> = source_code.chars().collect();
        let current_char = chars.get(0).copied();

        Lexer {
            input: chars,
            index: 0,
            position: Position { file: filename, line: 1, col: 1 },
            current_char: current_char,
            diagnostics: vec![],
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens: Vec<Token> = vec![];

        while let Some(ch) = self.current_char {
            match ch {
                ' ' | '\t' | '\r' => self.advance(),
                '\n' => {
                    self.advance();
                    self.position.line += 1;
                    self.position.col = 1;
                }
                '(' | '[' => {
                    tokens.push(Token {
                        ty: TokenType::LParen,
                        value: "".to_string(),
                        position: self.get_position(),
                    });

                    self.advance();
                }
                ')' | ']' => {
                    tokens.push(Token {
                        ty: TokenType::RParen,
                        value: "".to_string(),
                        position: self.get_position(),
                    });

                    self.advance();
                }
                '"' => match self.read_string() {
                    Some(string) => {
                        tokens.push(string);
                    }
                    None => {}
                },
                _ => {
                    let mut symbol = self.read_symbol();

                    if symbol.value.as_str() == "true" || symbol.value.as_str() == "false" {
                        symbol.ty = TokenType::BoolLiteral;
                    } else if is_keyword(&symbol.value) {
                        symbol.ty = TokenType::Keyword;
                    } else if let Ok(_) = &symbol.value.parse::<i64>() {
                        symbol.ty = TokenType::IntLiteral;
                    } else if let Ok(_) = &symbol.value.parse::<f64>() {
                        symbol.ty = TokenType::FloatLiteral;
                    }

                    tokens.push(symbol);
                }
            }
        }

        return tokens;
    }

    fn read_string(&mut self) -> Option<Token> {
        let pos = self.get_position();
        self.advance();
        let mut value = String::new();

        while let Some(ch) = self.current_char {
            match ch {
                '"' => {
                    self.advance();
                    return Some(Token {
                        ty: TokenType::StringLiteral,
                        value: value,
                        position: pos,
                    });
                }
                '\\' => {
                    self.advance();

                    match self.current_char {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('r') => value.push('\r'),
                        Some('\\') => value.push('\\'),
                        Some('"') => value.push('"'),
                        Some(c) => value.push(c),
                        None => {
                            self.diagnostics.push(Diagnostic::new(self.get_position(), "Unterminated string literal"));
                            self.advance();

                            return None;
                        }
                    }

                    self.advance();
                }
                '\n' => {
                    self.advance();
                    self.diagnostics.push(Diagnostic::new(pos, "Unterminated string literal"));

                    return None;
                }
                _ => {
                    value.push(ch);
                    self.advance();
                }
            }
        }

        self.diagnostics.push(Diagnostic::new(self.get_position(), "Unterminated string literal"));

        None
    }

    fn read_symbol(&mut self) -> Token {
        let mut symbol = String::new();
        let pos = self.get_position();

        while let Some(ch) = self.current_char {
            match ch {
                ' ' | '\t' | '\n' | '\r' | '(' | ')' | '[' | ']' | '"' => break,
                _ => {
                    symbol.push(ch);
                    self.advance();
                }
            }
        }

        Token {
            ty: TokenType::Symbol,
            value: symbol,
            position: pos,
        }
    }

    fn get_position(&self) -> Position {
        self.position.clone()
    }

    fn advance(&mut self) {
        self.index += 1;
        self.position.col += 1;
        self.current_char = self.input.get(self.index).copied();
    }
}
