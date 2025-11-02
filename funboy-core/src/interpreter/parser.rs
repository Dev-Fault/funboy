use std::mem::size_of;
use std::str::FromStr;

use crate::interpreter::lexer::KEYWORD_FALSE;
use crate::interpreter::lexer::KEYWORD_TRUE;

use super::lexer::Token;
use super::lexer::TokenType;

pub const COMMAND_STACK_EXPECT: &str = "Command stack should have at least one command";
pub const TOKEN_VALUE_EXCEPT: &str = "Token must have value";
pub const ERR_LOCATION_WIDTH: usize = 3;

// General purpose
pub const PRINT: &str = "print";

// Numbers
pub const ADD: &str = "add";
pub const SUBTRACT: &str = "sub";
pub const MULTIPLY: &str = "mul";
pub const DIVIDE: &str = "div";
pub const MOD: &str = "mod";
pub const RANDOM_RANGE: &str = "random_range";

// Variables
pub const COPY: &str = "copy";
pub const PASTE: &str = "paste";

// Booleans
pub const EQ: &str = "eq";
pub const GT: &str = "gt";
pub const LT: &str = "lt";
pub const NOT: &str = "not";
pub const AND: &str = "and";
pub const OR: &str = "or";

// Text
pub const CAPITALIZE: &str = "capitalize";
pub const UPPER: &str = "upper";
pub const LOWER: &str = "lower";
pub const REMOVE_WHITESPACE: &str = "remove_whitespace";
pub const CONCATENATE: &str = "concat";
pub const STARTS_WITH: &str = "starts_with";
pub const ENDS_WITH: &str = "ends_with";
pub const NEW_LINE: &str = "nl";
pub const SELECT_RANDOM: &str = "select_random";
pub const GET_SUB: &str = "get_sub";

// Lists and Text
pub const INDEX: &str = "index";
pub const SLICE: &str = "slice";
pub const LENGTH: &str = "length";
pub const SWAP: &str = "swap";
pub const INSERT: &str = "insert";
pub const REMOVE: &str = "remove";
pub const REPLACE: &str = "replace";

// Control flow
pub const IF_THEN: &str = "if_then";
pub const IF_THEN_ELSE: &str = "if_then_else";
pub const REPEAT: &str = "repeat";
pub const WHILE: &str = "while";

#[derive(Debug, PartialEq, Clone)]
pub enum CommandType {
    Add,
    Subtract,
    Multiply,
    Divide,
    Mod,
    SelectRandom,
    RandomRange,
    Capitalize,
    Upper,
    Lower,
    RemoveWhitespace,
    Repeat,
    While,
    Copy,
    Paste,
    Print,
    Concatenate,
    IfThen,
    IfThenElse,
    Not,
    And,
    Or,
    Eq,
    Gt,
    Lt,
    StartsWith,
    EndsWith,
    NewLine,
    Index,
    Slice,
    Length,
    Swap,
    Insert,
    Remove,
    Replace,
}

impl CommandType {
    pub fn to_str(&self) -> &str {
        match self {
            CommandType::Add => ADD,
            CommandType::Subtract => SUBTRACT,
            CommandType::Multiply => MULTIPLY,
            CommandType::Divide => DIVIDE,
            CommandType::SelectRandom => SELECT_RANDOM,
            CommandType::RandomRange => RANDOM_RANGE,
            CommandType::Capitalize => CAPITALIZE,
            CommandType::Upper => UPPER,
            CommandType::Lower => LOWER,
            CommandType::Repeat => REPEAT,
            CommandType::Copy => COPY,
            CommandType::Paste => PASTE,
            CommandType::Print => PRINT,
            CommandType::RemoveWhitespace => REMOVE_WHITESPACE,
            CommandType::Concatenate => CONCATENATE,
            CommandType::IfThen => IF_THEN,
            CommandType::IfThenElse => IF_THEN_ELSE,
            CommandType::Not => NOT,
            CommandType::And => AND,
            CommandType::Or => OR,
            CommandType::Eq => EQ,
            CommandType::Gt => GT,
            CommandType::Lt => LT,
            CommandType::StartsWith => STARTS_WITH,
            CommandType::EndsWith => ENDS_WITH,
            CommandType::NewLine => NEW_LINE,
            CommandType::Mod => MOD,
            CommandType::While => WHILE,
            CommandType::Index => INDEX,
            CommandType::Slice => SLICE,
            CommandType::Length => LENGTH,
            CommandType::Swap => SWAP,
            CommandType::Insert => INSERT,
            CommandType::Remove => REMOVE,
            CommandType::Replace => REPLACE,
        }
    }

    pub fn gen_err(&self, description: &str) -> String {
        format!(
            "Semantic\nCommand: {}\nDescription: {}",
            self.to_str(),
            description
        )
    }
}

impl FromStr for CommandType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ADD => Ok(CommandType::Add),
            SUBTRACT => Ok(CommandType::Subtract),
            MULTIPLY => Ok(CommandType::Multiply),
            DIVIDE => Ok(CommandType::Divide),
            MOD => Ok(CommandType::Mod),
            SELECT_RANDOM => Ok(CommandType::SelectRandom),
            RANDOM_RANGE => Ok(CommandType::RandomRange),
            CAPITALIZE => Ok(CommandType::Capitalize),
            UPPER => Ok(CommandType::Upper),
            LOWER => Ok(CommandType::Lower),
            REPEAT => Ok(CommandType::Repeat),
            WHILE => Ok(CommandType::While),
            COPY => Ok(CommandType::Copy),
            PASTE => Ok(CommandType::Paste),
            PRINT => Ok(CommandType::Print),
            REMOVE_WHITESPACE => Ok(CommandType::RemoveWhitespace),
            CONCATENATE => Ok(CommandType::Concatenate),
            IF_THEN => Ok(CommandType::IfThen),
            IF_THEN_ELSE => Ok(CommandType::IfThenElse),
            NOT => Ok(CommandType::Not),
            AND => Ok(CommandType::And),
            OR => Ok(CommandType::Or),
            EQ => Ok(CommandType::Eq),
            GT => Ok(CommandType::Gt),
            LT => Ok(CommandType::Lt),
            STARTS_WITH => Ok(CommandType::StartsWith),
            ENDS_WITH => Ok(CommandType::EndsWith),
            NEW_LINE => Ok(CommandType::NewLine),
            INDEX => Ok(CommandType::Index),
            SLICE => Ok(CommandType::Slice),
            LENGTH => Ok(CommandType::Length),
            SWAP => Ok(CommandType::Swap),
            INSERT => Ok(CommandType::Insert),
            REMOVE => Ok(CommandType::Remove),
            REPLACE => Ok(CommandType::Replace),
            _ => Err(format!("Invalid command {}", s)),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ValueType {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<ValueType>),
    Identifier(String),
    Command(Command),
    None,
}

impl ValueType {
    pub fn extract_float(&self) -> Option<f64> {
        match self {
            ValueType::Int(value) => Some(*value as f64),
            ValueType::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub fn extract_int(&self) -> Option<i64> {
        match self {
            ValueType::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_size(&self) -> usize {
        match self {
            ValueType::Text(value) => size_of::<ValueType>() + value.capacity(),
            ValueType::Int(_) => size_of::<ValueType>(),
            ValueType::Float(_) => size_of::<ValueType>(),
            ValueType::Bool(_) => size_of::<ValueType>(),
            ValueType::List(values) => size_of::<ValueType>()
                .saturating_add(values.iter().map(|value| value.get_size()).sum()),
            ValueType::Identifier(value) => size_of::<ValueType>() + value.capacity(),
            ValueType::Command(value) => size_of::<ValueType>()
                .saturating_add(value.args.iter().map(|value| value.get_size()).sum()),
            ValueType::None => size_of::<ValueType>(),
        }
    }
}

impl ToString for ValueType {
    fn to_string(&self) -> String {
        match self {
            ValueType::Text(value) => value.to_string(),
            ValueType::Int(value) => value.to_string(),
            ValueType::Float(value) => value.to_string(),
            ValueType::Bool(value) => value.to_string(),
            ValueType::List(values) => {
                let list_string: String = values
                    .iter()
                    .map(|value| value.to_string() + ", ")
                    .collect();
                format!("[{}]", &list_string[0..list_string.len() - 2])
            }
            ValueType::Identifier(value) => value.to_string(),
            ValueType::Command(value) => value.command_type.to_str().to_string(),
            ValueType::None => "".to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Command {
    pub command_type: CommandType,
    pub args: Vec<ValueType>,
}

impl Command {
    fn from(command_name: &str) -> Result<Command, String> {
        let command_type = CommandType::from_str(command_name)?;

        Ok(Command {
            command_type,
            args: Vec::new(),
        })
    }
}

struct TokenIndex<'a> {
    index: usize,
    tokens: &'a Vec<Token>,
}

impl<'a> TokenIndex<'a> {
    pub fn comes_after(&self, tokens: &[TokenType]) -> bool {
        match self.tokens.get(self.index.saturating_sub(1)) {
            Some(token) => tokens.contains(&token.token_type),
            None => false,
        }
    }

    pub fn comes_before(&self, tokens: &[TokenType]) -> bool {
        match self.tokens.get(self.index + 1) {
            Some(token) => tokens.contains(&token.token_type),
            None => false,
        }
    }

    pub fn gen_err(&self, description: &str) -> String {
        let mut err = "Syntax\nLocation: ".to_string();

        let left = self.index.saturating_sub(ERR_LOCATION_WIDTH);
        let right = match self.index + ERR_LOCATION_WIDTH > self.tokens.len() {
            true => self.tokens.len(),
            false => self.index + ERR_LOCATION_WIDTH,
        };

        for token in &self.tokens[left..right] {
            err.push_str(&token.value);
        }

        err.push_str(&format!("\nDescription: {}", description.to_string()));

        err
    }
}

struct CommandIndex<'a> {
    command: Command,
    token_index: TokenIndex<'a>,
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Command>, String> {
    let mut commands: Vec<Command> = Vec::new();

    let mut command_stack: Vec<CommandIndex> = Vec::new();

    for (i, token) in tokens.iter().enumerate() {
        let token_index = TokenIndex {
            index: i,
            tokens: &tokens,
        };

        match token.token_type {
            TokenType::OpeningQuote => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Opening quote outside of a command"));
                } else if !token_index
                    .comes_after(&[TokenType::Comma, TokenType::OpeningParenthesis])
                {
                    return Err(token_index.gen_err("Misplaced opening quote"));
                }
            }
            TokenType::ClosingQuote => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Closing quote outside of a command"));
                } else if !token_index
                    .comes_before(&[TokenType::Comma, TokenType::ClosingParenthesis])
                {
                    return Err(token_index.gen_err("Misplaced closing quote"));
                } else if !token_index.comes_after(&[TokenType::Text]) {
                    return Err(token_index.gen_err("Misplace closing quote"));
                }
            }
            TokenType::OpeningParenthesis => {
                if !token_index.comes_after(&[TokenType::Command]) {
                    return Err(
                        token_index.gen_err("Opening parenthesis must come after a command")
                    );
                }
            }
            TokenType::ClosingParenthesis => match command_stack.pop() {
                Some(command_index) => {
                    if command_stack.len() == 0 {
                        commands.push(command_index.command)
                    } else {
                        command_stack
                            .last_mut()
                            .expect(COMMAND_STACK_EXPECT)
                            .command
                            .args
                            .push(ValueType::Command(command_index.command));
                    }
                }
                None => {
                    return Err(
                        token_index.gen_err("Closing parenthesis must close a commands arguments")
                    );
                }
            },
            TokenType::Comma => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Comma outside of a command"));
                }
            }
            TokenType::Command => {
                if !token_index.comes_before(&[TokenType::OpeningParenthesis]) {
                    return Err(token_index.gen_err("Command must come before opening parenthesis"));
                } else if token.value.is_empty() {
                    return Err(
                        token_index.gen_err("Opening parenthesis must come after a command")
                    );
                } else {
                    command_stack.push(CommandIndex {
                        command: Command::from(&token.value)?,
                        token_index,
                    });
                }
            }
            TokenType::Text => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Text outside of command"));
                } else if !token_index.comes_after(&[TokenType::OpeningQuote]) {
                    return Err(token_index.gen_err("Text must come after opening quotes"));
                } else if !token_index.comes_before(&[TokenType::ClosingQuote]) {
                    return Err(token_index.gen_err("Text must come before closing quotes"));
                } else {
                    command_stack
                        .last_mut()
                        .expect(COMMAND_STACK_EXPECT)
                        .command
                        .args
                        .push(ValueType::Text(token.value.clone()));
                }
            }
            TokenType::Number => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Number outside of command"));
                } else if !token_index
                    .comes_after(&[TokenType::Comma, TokenType::OpeningParenthesis])
                {
                    return Err(token_index.gen_err("Number outside of command"));
                } else {
                    if let Ok(value) = token.value.parse::<i64>() {
                        command_stack
                            .last_mut()
                            .expect(COMMAND_STACK_EXPECT)
                            .command
                            .args
                            .push(ValueType::Int(value));
                    } else {
                        let value = token
                            .value
                            .parse::<f64>()
                            .expect("Number token type must be parsable");

                        command_stack
                            .last_mut()
                            .expect(COMMAND_STACK_EXPECT)
                            .command
                            .args
                            .push(ValueType::Float(value));
                    }
                }
            }
            TokenType::Identifier => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Identifier outside of command"));
                } else if !token_index
                    .comes_after(&[TokenType::Comma, TokenType::OpeningParenthesis])
                {
                    return Err(token_index.gen_err("Identifier outside of command"));
                } else {
                    command_stack
                        .last_mut()
                        .expect(COMMAND_STACK_EXPECT)
                        .command
                        .args
                        .push(ValueType::Identifier(token.value.clone()));
                }
            }
            TokenType::Keyword => {
                if command_stack.len() == 0 {
                    return Err(token_index.gen_err("Keyword outside of command"));
                } else if !token_index
                    .comes_after(&[TokenType::Comma, TokenType::OpeningParenthesis])
                {
                    return Err(token_index.gen_err("Keyword outside of command"));
                } else {
                    let value_type = match &token.value[..] {
                        KEYWORD_TRUE => ValueType::Bool(true),
                        KEYWORD_FALSE => ValueType::Bool(false),
                        _ => {
                            return Err(token_index.gen_err("Non existant keyword"));
                        }
                    };

                    command_stack
                        .last_mut()
                        .expect(COMMAND_STACK_EXPECT)
                        .command
                        .args
                        .push(value_type);
                }
            }
        }
    }

    if command_stack.len() != 0 {
        let command_index = command_stack.last().expect(COMMAND_STACK_EXPECT);

        let description = format!(
            "Missing closing parenthesis within command **{}**",
            command_index.command.command_type.to_str()
        );
        return Err(command_index.token_index.gen_err(&description));
    } else {
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {

    use crate::interpreter::{
        lexer::tokenize,
        parser::{Command, CommandType, ValueType},
    };

    use super::parse;

    #[test]
    fn simple_parse() {
        let code = "add(5, 10)";

        let commands = parse(tokenize(code)).unwrap();

        assert_eq!(commands[0].command_type, CommandType::Add);
        assert_eq!(
            commands[0].args,
            vec![ValueType::Int(5), ValueType::Int(10)]
        );

        // dbg!(commands);
    }

    #[test]
    fn logical_parse() {
        let code = "if_then(true, print(\"true\"))";

        let commands = parse(tokenize(code)).unwrap();

        assert_eq!(commands[0].command_type, CommandType::IfThen);
        assert_eq!(
            commands[0].args,
            vec![
                ValueType::Bool(true),
                ValueType::Command(Command {
                    command_type: CommandType::Print,
                    args: vec![ValueType::Text("true".to_string())],
                })
            ]
        )
    }

    #[test]
    fn nested_parse() {
        let code = "add(add(identifier, \"text\"), add(add(1, 2), 2))";

        let commands = parse(tokenize(code)).unwrap();

        assert_eq!(commands[0].command_type, CommandType::Add);
        assert_eq!(
            commands[0].args,
            vec![
                ValueType::Command(Command {
                    command_type: CommandType::Add,
                    args: vec![
                        ValueType::Identifier("identifier".to_string()),
                        ValueType::Text("text".to_string())
                    ],
                }),
                ValueType::Command(Command {
                    command_type: CommandType::Add,
                    args: vec![
                        ValueType::Command(Command {
                            command_type: CommandType::Add,
                            args: vec![ValueType::Int(1), ValueType::Int(2)]
                        }),
                        ValueType::Int(2)
                    ]
                })
            ]
        );
        // dbg!(commands);
    }
}
