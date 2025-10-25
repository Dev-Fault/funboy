use core::panic;

pub const COMMA: &str = ",";
pub const OPENING_PARENTHESIS: &str = "(";
pub const CLOSING_PARENTHESIS: &str = ")";
pub const QUOTE: &str = "\"";
pub const ESCAPE: &str = "\\";
pub const ESCAPED_QUOTE: &str = "\\\"";
pub const KEYWORD_TRUE: &str = "true";
pub const KEYWORD_FALSE: &str = "false";

pub const SYMBOLS: [&str; 4] = [",", "(", ")", "\""];
pub const QUOTE_SYMBOLS: [&str; 2] = [ESCAPED_QUOTE, QUOTE];
pub const KEYWORDS: [&str; 2] = [KEYWORD_TRUE, KEYWORD_FALSE];

#[derive(Debug, PartialEq)]
pub enum TokenType {
    OpeningQuote,
    ClosingQuote,
    OpeningParenthesis,
    ClosingParenthesis,
    Comma,
    Command,
    Text,
    Number,
    Identifier,
    Keyword,
}

#[derive(Debug)]
pub struct Token {
    pub token_type: TokenType,
    pub value: String,
}

pub fn tokenize(code: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    let mut inside_quote = false;
    let mut incomplete_string: Option<String> = None;

    for c in code.chars() {
        buffer.push(c);

        let symbols: &[&str];

        if inside_quote {
            symbols = &QUOTE_SYMBOLS;
        } else {
            symbols = &SYMBOLS;
        }

        if let Some(symbol) = get_symbol(&buffer, &symbols) {
            let (left, _right) = buffer.split_once(symbol).unwrap();

            match symbol {
                ESCAPED_QUOTE if inside_quote => {
                    let split = buffer.split_once(ESCAPE).unwrap();
                    let text = format!("{}{}", split.0.to_string(), split.1.to_string());
                    match incomplete_string {
                        Some(ref mut str) => str.push_str(&text),
                        None => {
                            incomplete_string = Some(text);
                        }
                    }
                }
                QUOTE => {
                    if inside_quote {
                        let mut text = String::new();
                        inside_quote = false;

                        if let Some(ref s) = incomplete_string {
                            text.push_str(&s);
                            incomplete_string = None;
                        }

                        text.push_str(left);

                        tokens.push(Token {
                            token_type: TokenType::Text,
                            value: text,
                        });
                        tokens.push(Token {
                            token_type: TokenType::ClosingQuote,
                            value: QUOTE.to_string(),
                        });
                    } else {
                        inside_quote = true;

                        tokens.push(Token {
                            token_type: TokenType::OpeningQuote,
                            value: QUOTE.to_string(),
                        });
                    }
                }
                OPENING_PARENTHESIS if !inside_quote => {
                    tokens.push(Token {
                        token_type: TokenType::Command,
                        value: left.trim().to_string(),
                    });

                    tokens.push(Token {
                        token_type: TokenType::OpeningParenthesis,
                        value: OPENING_PARENTHESIS.to_string(),
                    });
                }
                CLOSING_PARENTHESIS if !inside_quote => {
                    if buffer != CLOSING_PARENTHESIS {
                        let token_type;

                        if left.trim().parse::<f64>().is_ok() {
                            token_type = TokenType::Number
                        } else if KEYWORDS.contains(&left.trim()) {
                            token_type = TokenType::Keyword
                        } else {
                            token_type = TokenType::Identifier
                        }

                        tokens.push(Token {
                            token_type,
                            value: left.trim().to_string(),
                        });
                    }

                    tokens.push(Token {
                        token_type: TokenType::ClosingParenthesis,
                        value: CLOSING_PARENTHESIS.to_string(),
                    });
                }
                COMMA if !inside_quote => {
                    if buffer != COMMA {
                        let token_type;

                        if left.trim().parse::<f64>().is_ok() {
                            token_type = TokenType::Number
                        } else if KEYWORDS.contains(&left.trim()) {
                            token_type = TokenType::Keyword
                        } else {
                            token_type = TokenType::Identifier
                        }

                        tokens.push(Token {
                            token_type,
                            value: left.trim().to_string(),
                        });
                    }

                    tokens.push(Token {
                        token_type: TokenType::Comma,
                        value: COMMA.to_string(),
                    });
                }
                _ => {
                    if !inside_quote {
                        panic!("Should never contain an invalid symbol.")
                    } else {
                        continue;
                    }
                }
            }
            buffer.clear();
        }
    }

    tokens
}

fn get_symbol<'a>(code: &str, symbols: &[&'a str]) -> Option<&'a str> {
    for symbol in symbols {
        if code.contains(symbol) {
            return Some(symbol);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::interpreter::lexer::{OPENING_PARENTHESIS, TokenType, tokenize};

    #[test]
    fn standard_tokens() {
        let code = "add(\"'noun\", \"'puncuation\") multiply(2, identifier)";

        let tokens = tokenize(code);

        assert_eq!(tokens[0].token_type, TokenType::Command);
        assert_eq!(tokens[0].value, "add".to_string());

        assert_eq!(tokens[1].token_type, TokenType::OpeningParenthesis);

        assert_eq!(tokens[2].token_type, TokenType::OpeningQuote);

        assert_eq!(tokens[3].token_type, TokenType::Text);
        assert_eq!(tokens[3].value, "'noun".to_string());

        assert_eq!(tokens[4].token_type, TokenType::ClosingQuote);

        assert_eq!(tokens[5].token_type, TokenType::Comma);

        assert_eq!(tokens[6].token_type, TokenType::OpeningQuote);

        assert_eq!(tokens[7].token_type, TokenType::Text);
        assert_eq!(tokens[7].value, "'puncuation".to_string());

        assert_eq!(tokens[8].token_type, TokenType::ClosingQuote);

        assert_eq!(tokens[9].token_type, TokenType::ClosingParenthesis);

        assert_eq!(tokens[10].token_type, TokenType::Command);
        assert_eq!(tokens[10].value, "multiply".to_string());

        assert_eq!(tokens[11].token_type, TokenType::OpeningParenthesis);

        assert_eq!(tokens[12].token_type, TokenType::Number);
        assert_eq!(tokens[12].value, "2".to_string());

        assert_eq!(tokens[13].token_type, TokenType::Comma);

        assert_eq!(tokens[14].token_type, TokenType::Identifier);
        assert_eq!(tokens[14].value, "identifier".to_string());

        assert_eq!(tokens[15].token_type, TokenType::ClosingParenthesis);

        // dbg!(tokens);
    }

    #[test]
    fn escaped_quotes() {
        let code = "print(\"\\\"qu\\\"o\\\"te\\\"\")";
        let tokens = tokenize(code);
        dbg!(&tokens[3]);
        assert_eq!(tokens[3].token_type, TokenType::Text);
        assert_eq!(tokens[3].value, "\"qu\"o\"te\"".to_string());
    }

    #[test]
    fn symbols_inside_quotes() {
        let code = "print(\" example of, some, (), symbols \\\" inside, of quotes \")";

        let tokens = tokenize(code);

        assert_eq!(tokens[0].token_type, TokenType::Command);
        assert_eq!(tokens[0].value, "print".to_string());

        assert_eq!(tokens[1].token_type, TokenType::OpeningParenthesis);

        assert_eq!(tokens[2].token_type, TokenType::OpeningQuote);

        assert_eq!(tokens[3].token_type, TokenType::Text);
        assert_eq!(
            tokens[3].value,
            " example of, some, (), symbols \" inside, of quotes ".to_string()
        );

        assert_eq!(tokens[4].token_type, TokenType::ClosingQuote);

        assert_eq!(tokens[5].token_type, TokenType::ClosingParenthesis);

        // dbg!(tokens);
    }

    #[test]
    fn keyword_token() {
        let code = "print(true)";
        let tokens = tokenize(code);
        assert_eq!(tokens[0].token_type, TokenType::Command);
        assert_eq!(tokens[0].value, "print".to_string());

        assert_eq!(tokens[1].token_type, TokenType::OpeningParenthesis);

        assert_eq!(tokens[2].token_type, TokenType::Keyword);
        assert_eq!(tokens[2].value, "true".to_string());

        assert_eq!(tokens[3].token_type, TokenType::ClosingParenthesis);
    }
}
