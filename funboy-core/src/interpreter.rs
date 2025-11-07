use async_recursion::async_recursion;
use lexer::tokenize;
use parser::{Command, CommandType, ValueType, parse};
use rand::{self, Rng};
use std::collections::HashMap;

pub mod documentation;
#[allow(dead_code)]
mod lexer;
#[allow(dead_code)]
mod parser;

const LOOP_LIMIT: u16 = u16::MAX;
const VAR_MAP_BYTE_LIMIT: usize = 65535 * 100;
const OUTPUT_BYTE_LIMIT: usize = 8000;

const ERROR_NO_ARGS: &str = "takes no arguments";
const ERROR_TWO_OR_MORE_ARGS: &str = "must have two or more arguments";
const ERROR_EXACTLY_ONE_ARG: &str = "must have exactly one argument";
const ERROR_EXACTLY_TWO_ARGS: &str = "must have exactly two arguments";
const ERROR_EXACTLY_THREE_ARGS: &str = "must have exactly three arguments";
const ERROR_ARGS_MUST_BE_NUMBER: &str = "all arguments must be of type Number";
const ERROR_ARGS_MUST_BE_BOOL: &str = "all arguments must be of type Bool";
const ERROR_ARGS_MUST_BE_TEXT: &str = "all arguments must be of type Text";
const ERROR_ARG_MUST_BE_TEXT: &str = "argument must be of type Text";
// const ERROR_ARG_MUST_BE_NUMBER: &str = "argument must be of type Number";
const ERROR_ARG_MUST_BE_IDENTIFIER: &str = "argument must be of type Identifier";
const ERROR_ARG_ONE_MUST_BE_WHOLE_NUMBER: &str = "first argument must be a whole number";
const ERROR_ARG_ONE_MUST_BE_BOOL: &str = "first argument must be of type Bool";
const ERROR_ARG_ONE_MUST_BE_COMMAND_BOOL: &str =
    "first argument must be a command that evaluates to a boolean value";
// const ERROR_ARG_ONE_MUST_BE_COMMAND: &str = "first argument must be of type Command";

const ERROR_ARGS_AFTER_ARG_ONE_MUST_BE_COMMAND: &str =
    "arguments following first argument must be of type Command";
const ERROR_ARG_ONE_MUST_NOT_BE_IDENTIFIER: &str = "first argument must not be of type Identifier";
const ERROR_ARG_ONE_MUST_NOT_BE_NONE: &str = "first argument must not be of type None";
const ERROR_ARG_TWO_MUST_BE_IDENTIFIER: &str = "second argument must be of type Identifier";
const ERROR_UNKNOWN_IDENTIFIER: &str = "no identifier exists named";
const ERROR_ZERO_DIVISION: &str = "division by zero";

#[derive(Debug)]
pub struct VarMap {
    data: HashMap<String, ValueType>,
    size: usize,
}

impl VarMap {
    pub fn new() -> Self {
        VarMap {
            data: HashMap::new(),
            size: 0,
        }
    }

    pub fn insert_var(&mut self, name: String, value: ValueType) -> Result<(), String> {
        if self.size.saturating_add(value.get_size()) <= VAR_MAP_BYTE_LIMIT {
            self.data.insert(name, value);
            Ok(())
        } else {
            Err(format!(
                "interpreter memory limit of {} bytes exceeded",
                VAR_MAP_BYTE_LIMIT
            ))
        }
    }

    pub fn get_var_mut(&mut self, name: &String) -> Option<&mut ValueType> {
        self.data.get_mut(name)
    }

    pub fn get_var(&self, name: &String) -> Option<&ValueType> {
        self.data.get(name)
    }

    pub fn get_var_or_err(&self, name: &String) -> Result<&ValueType, String> {
        match self.data.get(name) {
            Some(value) => Ok(value),
            None => Err(format!("identifier {} did not contain a value", name)),
        }
    }

    pub fn get_var_or_err_mut(&mut self, name: &String) -> Result<&mut ValueType, String> {
        match self.data.get_mut(name) {
            Some(value) => Ok(value),
            None => Err(format!("identifier {} did not contain a value", name)),
        }
    }
}

impl ValueType {
    fn to_string(&self, var_map: &VarMap) -> Result<String, String> {
        let result = match self {
            ValueType::Text(value) => value.to_string(),
            ValueType::Int(value) => value.to_string(),
            ValueType::Float(value) => value.to_string(),
            ValueType::Bool(value) => value.to_string(),
            ValueType::List(values) => {
                let list_string: String = values
                    .iter()
                    .map(|value| match value.to_string(var_map) {
                        Ok(value) => value + ", ",
                        Err(e) => {
                            return e;
                        }
                    })
                    .collect();
                format!("[{}]", &list_string[0..list_string.len() - 2])
            }
            ValueType::Identifier(name) => {
                let value = var_map.get_var_or_err(name)?;
                value.to_string(var_map)?
            }
            ValueType::Command(value) => value.command_type.to_str().to_string(),
            ValueType::None => "".to_string(),
        };
        Ok(result)
    }

    /// Returns inner value if Identifier, otherwise returns value that was passed in
    pub fn get_inner_value<'a>(&'a self, var_map: &'a VarMap) -> Result<&'a ValueType, String> {
        match self {
            ValueType::Identifier(name) => Ok(var_map.get_var_or_err(name)?),
            _ => Ok(self),
        }
    }
}

pub trait ContainsFloat {
    fn contains_float(&self, var_map: &VarMap) -> bool;
}

impl ContainsFloat for Vec<ValueType> {
    fn contains_float(&self, var_map: &VarMap) -> bool {
        for arg in self {
            if let ValueType::Float(_) = arg {
                return true;
            } else if let ValueType::Identifier(name) = arg {
                match var_map.get_var(name) {
                    Some(value) => {
                        if let ValueType::Float(_) = value {
                            return true;
                        }
                    }
                    None => continue,
                };
            }
        }
        return false;
    }
}

pub trait ExtractValue {
    fn extract_float(&self, var_map: &VarMap) -> Option<f64>;
    fn extract_int(&self, var_map: &VarMap) -> Option<i64>;
}

impl ExtractValue for ValueType {
    fn extract_float(&self, var_map: &VarMap) -> Option<f64> {
        match self {
            ValueType::Int(value) => Some(*value as f64),
            ValueType::Float(value) => Some(*value),
            ValueType::Identifier(name) => match var_map.get_var(name) {
                Some(value) => value.extract_float(var_map),
                None => None,
            },
            _ => None,
        }
    }

    fn extract_int(&self, var_map: &VarMap) -> Option<i64> {
        match self {
            ValueType::Int(value) => Some(*value),
            ValueType::Identifier(name) => match var_map.get_var(name) {
                Some(value) => value.extract_int(var_map),
                None => None,
            },
            _ => None,
        }
    }
}

pub trait Capitalize {
    fn capitalize(&self) -> String;
}

impl Capitalize for String {
    fn capitalize(&self) -> String {
        if self.len() < 1 {
            self.to_string()
        } else {
            format!("{}{}", self[0..1].to_uppercase(), self[1..].to_string())
        }
    }
}

#[derive(Debug)]
pub struct Interpreter {
    vars: VarMap,
    copy_buffer: ValueType,
    output: String,
    log: Vec<ValueType>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            copy_buffer: ValueType::None,
            vars: VarMap::new(),
            output: String::new(),
            log: Vec::new(),
        }
    }

    pub async fn interpret_embedded_code(&mut self, input: &str) -> Result<String, String> {
        let mut output = String::with_capacity(input.len());
        let mut code_stack: Vec<String> = Vec::new();

        let mut code_depth: i16 = 0;

        for c in input.chars() {
            if c == '{' {
                code_stack.push(String::new());
                code_depth += 1;
            } else if c == '}' {
                code_depth -= 1;
                if code_depth < 0 {
                    return Err("Unmatched curly braces".to_string());
                } else {
                    match code_stack.pop() {
                        Some(code) => {
                            match self.interpret(&code).await {
                                Ok(eval) => match code_stack.last_mut() {
                                    Some(code) => code.push_str(&eval),
                                    None => output.push_str(&eval),
                                },
                                Err(e) => return Err(e),
                            };
                        }
                        None => {}
                    }
                }
            } else if code_depth == 0 {
                output.push(c);
            } else {
                match code_stack.last_mut() {
                    Some(s) => s.push(c),
                    None => {}
                }
            }
        }

        if code_depth != 0 {
            return Err("Unmatched curly braces".to_string());
        }

        Ok(output)
    }

    pub async fn interpret(&mut self, code: &str) -> Result<String, String> {
        let commands = parse(tokenize(code))?;

        let mut final_value = ValueType::None;

        for command in commands {
            final_value = self.eval_command(command).await?;
        }

        match final_value {
            ValueType::List(_) => {}
            ValueType::Command(_) => {}
            ValueType::None => {}
            _ => self.output.push_str(&final_value.to_string(&self.vars)?),
        }

        Ok(self.output.drain(..).collect())
    }

    pub async fn interpret_and_log(&mut self, code: &str) -> Result<String, String> {
        let commands = parse(tokenize(code))?;

        for command in commands {
            let value = self.eval_command(command).await?;
            self.log.push(value);
        }

        Ok(self.output.drain(..).collect())
    }

    #[async_recursion]
    async fn eval_command(&mut self, command: Command) -> Result<ValueType, String> {
        let mut args: Vec<ValueType> = Vec::new();
        let mut i = 0;

        for arg in command.args {
            match arg {
                ValueType::Command(ref sub_command) => {
                    match command.command_type {
                        CommandType::IfThen if i == 1 => args.push(arg),
                        CommandType::IfThenElse if i == 1 || i == 2 => args.push(arg),
                        CommandType::Repeat if i != 0 => args.push(arg),
                        CommandType::While => args.push(arg),
                        _ => args.push(self.eval_command(sub_command.clone()).await?),
                    };
                }
                ValueType::Text(_) => args.push(arg),
                ValueType::Int(_) => args.push(arg),
                ValueType::Float(_) => args.push(arg),
                ValueType::Identifier(_) => args.push(arg),
                ValueType::None => args.push(arg),
                ValueType::Bool(_) => args.push(arg),
                ValueType::List(_) => args.push(arg),
            }
            i += 1;
        }

        let command_type = &command.command_type;

        match command_type {
            CommandType::Add => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else if args.contains_float(&self.vars) {
                    if let Some(mut sum) = args[0].extract_float(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_float(&self.vars) {
                                Some(value) => sum += value,
                                None => return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER)),
                            }
                        }
                        Ok(ValueType::Float(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                } else {
                    if let Some(mut sum) = args[0].extract_int(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_int(&self.vars) {
                                Some(value) => sum += value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Int(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                }
            }
            CommandType::Subtract => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else if args.contains_float(&self.vars) {
                    if let Some(mut diff) = args[0].extract_float(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_float(&self.vars) {
                                Some(value) => diff -= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Float(diff))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                } else {
                    if let Some(mut diff) = args[0].extract_int(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_int(&self.vars) {
                                Some(value) => diff -= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Int(diff))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                }
            }
            CommandType::Multiply => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else if args.contains_float(&self.vars) {
                    if let Some(mut sum) = args[0].extract_float(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_float(&self.vars) {
                                Some(value) => sum *= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Float(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                } else {
                    if let Some(mut sum) = args[0].extract_int(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_int(&self.vars) {
                                Some(value) => sum *= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Int(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                }
            }
            CommandType::Divide => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else if args.contains_float(&self.vars) {
                    if let Some(mut sum) = args[0].extract_float(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_float(&self.vars) {
                                Some(value) => sum /= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Float(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                } else {
                    if let Some(mut sum) = args[0].extract_int(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_int(&self.vars) {
                                Some(value) => {
                                    if value == 0 {
                                        return Err(command_type.gen_err(ERROR_ZERO_DIVISION));
                                    } else {
                                        sum /= value
                                    }
                                }
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Int(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                }
            }
            CommandType::SelectRandom => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else {
                    let mut rng = rand::rng();
                    let index = rng.random_range(0..args.len());
                    Ok(args[index].clone())
                }
            }
            CommandType::RandomRange => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let mut rng = rand::rng();
                    if args.contains_float(&self.vars) {
                        if let (Some(min), Some(max)) = (
                            &args[0].extract_float(&self.vars),
                            &args[1].extract_float(&self.vars),
                        ) {
                            return Ok(ValueType::Float(rng.random_range(*min..*max)));
                        } else {
                            return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                        }
                    } else {
                        if let (Some(min), Some(max)) = (
                            &args[0].extract_int(&self.vars),
                            &args[1].extract_int(&self.vars),
                        ) {
                            return Ok(ValueType::Int(rng.random_range(*min..*max)));
                        } else {
                            return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                        }
                    }
                }
            }
            CommandType::Capitalize => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    match &args[0] {
                        ValueType::Text(text) => Ok(ValueType::Text(text.capitalize())),
                        ValueType::Identifier(_) => {
                            Ok(ValueType::Text(args[0].to_string(&self.vars)?.capitalize()))
                        }
                        _ => Err(command_type.gen_err(ERROR_ARG_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::Upper => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    match &args[0] {
                        ValueType::Text(text) => Ok(ValueType::Text(text.to_uppercase())),
                        ValueType::Identifier(_) => Ok(ValueType::Text(
                            args[0].to_string(&self.vars)?.to_uppercase(),
                        )),
                        _ => Err(command_type.gen_err(ERROR_ARG_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::Lower => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    match &args[0] {
                        ValueType::Text(text) => Ok(ValueType::Text(text.to_lowercase())),
                        ValueType::Identifier(_) => Ok(ValueType::Text(
                            args[0].to_string(&self.vars)?.to_lowercase(),
                        )),
                        _ => Err(command_type.gen_err(ERROR_ARG_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::RemoveWhitespace => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    match &args[0] {
                        ValueType::Text(text) => {
                            Ok(ValueType::Text(text.split_whitespace().collect()))
                        }
                        ValueType::Identifier(_) => Ok(ValueType::Text(
                            args[0].to_string(&self.vars)?.split_whitespace().collect(),
                        )),
                        _ => Err(command_type.gen_err(ERROR_ARG_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::Concatenate => {
                let mut concatenation = String::new();

                for arg in args {
                    concatenation.push_str(&arg.to_string(&self.vars)?);
                }

                Ok(ValueType::Text(concatenation))
            }
            CommandType::Repeat => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else {
                    match &args[0] {
                        ValueType::Int(value) => {
                            if *value > LOOP_LIMIT.into() {
                                return Err(command_type.gen_err(&format!(
                                    "must not exceed more than {} repetitions",
                                    LOOP_LIMIT
                                )));
                            }
                            for _i in 0..*value {
                                for arg in &args[1..args.len()] {
                                    if let ValueType::Command(command) = arg {
                                        self.eval_command(command.clone()).await?;
                                    } else {
                                        return Err(command_type
                                            .gen_err(ERROR_ARGS_AFTER_ARG_ONE_MUST_BE_COMMAND));
                                    };
                                }
                            }
                            return Ok(ValueType::None);
                        }
                        _ => {
                            return Err(command_type.gen_err(ERROR_ARG_ONE_MUST_BE_WHOLE_NUMBER));
                        }
                    }
                }
            }
            CommandType::Store => {
                if args.is_empty() {
                    return Err(command_type.gen_err("must have one or more arguments"));
                } else if args.len() == 1 {
                    match &args[0] {
                        ValueType::Identifier(_) => {
                            Err(command_type.gen_err(ERROR_ARG_ONE_MUST_NOT_BE_IDENTIFIER))
                        }
                        ValueType::None => {
                            Err(command_type.gen_err(ERROR_ARG_ONE_MUST_NOT_BE_NONE))
                        }
                        _ => {
                            self.copy_buffer = args[0].clone();
                            Ok(ValueType::None)
                        }
                    }
                } else if args.len() == 2 {
                    if let ValueType::Identifier(identifier) = &args[1] {
                        match &args[0] {
                            ValueType::Identifier(_) => {
                                Err(command_type.gen_err(ERROR_ARG_ONE_MUST_NOT_BE_IDENTIFIER))
                            }
                            ValueType::None => {
                                Err(command_type.gen_err(ERROR_ARG_ONE_MUST_NOT_BE_NONE))
                            }
                            _ => {
                                match self
                                    .vars
                                    .insert_var(identifier.to_string(), args[0].clone())
                                {
                                    Ok(_) => return Ok(ValueType::None),
                                    Err(e) => {
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    } else {
                        return Err(command_type.gen_err(ERROR_ARG_TWO_MUST_BE_IDENTIFIER));
                    }
                } else if let ValueType::Identifier(identifier) = &args[args.len() - 1] {
                    let mut list: Vec<ValueType> = Vec::new();
                    for arg in &args[0..args.len() - 1] {
                        match arg {
                            ValueType::Identifier(_) => {
                                return Err(
                                    command_type.gen_err("cannot store arg of type Identifier")
                                );
                            }
                            ValueType::None => {
                                return Err(command_type.gen_err("cannot store arg of type None"));
                            }
                            _ => list.push(arg.clone()),
                        };
                    }
                    match self
                        .vars
                        .insert_var(identifier.to_string(), ValueType::List(list))
                    {
                        Ok(_) => return Ok(ValueType::None),
                        Err(e) => {
                            return Err(e);
                        }
                    }
                } else {
                    return Err(command_type.gen_err("last arg must be of type Identifier"));
                }
            }
            CommandType::Clone => {
                if args.len() > 1 {
                    return Err(command_type.gen_err("cannot have more than 1 argument"));
                }
                if args.is_empty() {
                    Ok(self.copy_buffer.clone())
                } else {
                    match &args[0] {
                        ValueType::Identifier(identifier) => {
                            match self.vars.get_var_mut(identifier) {
                                Some(value) => Ok(value.clone()),
                                None => Err(command_type.gen_err(&format!(
                                    "{} **{}**",
                                    ERROR_UNKNOWN_IDENTIFIER, identifier
                                ))),
                            }
                        }
                        _ => Err(command_type.gen_err(ERROR_ARG_MUST_BE_IDENTIFIER)),
                    }
                }
            }
            CommandType::Print => {
                for arg in args {
                    let arg_string = arg.to_string(&self.vars)?;
                    if self.output.capacity().saturating_add(arg_string.capacity())
                        <= OUTPUT_BYTE_LIMIT
                    {
                        self.output.push_str(&arg_string);
                    } else {
                        return Err(format!(
                            "Output byte limit of {} bytes exceeded",
                            OUTPUT_BYTE_LIMIT
                        ));
                    }
                }

                Ok(ValueType::None)
            }
            CommandType::IfThen => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg = &args[0].get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Bool(bool) => {
                            if *bool {
                                match &args[1] {
                                    ValueType::Command(value) => {
                                        self.eval_command(value.clone()).await
                                    }
                                    _ => Ok(args[1].clone()),
                                }
                            } else {
                                Ok(ValueType::None)
                            }
                        }
                        _ => Err(command_type.gen_err(ERROR_ARG_ONE_MUST_BE_BOOL)),
                    }
                }
            }
            CommandType::IfThenElse => {
                if args.len() != 3 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_THREE_ARGS));
                } else {
                    let arg = &args[0].get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Bool(bool) => {
                            if *bool {
                                match &args[1] {
                                    ValueType::Command(value) => {
                                        self.eval_command(value.clone()).await
                                    }
                                    _ => Ok(args[1].clone()),
                                }
                            } else {
                                match &args[2] {
                                    ValueType::Command(value) => {
                                        self.eval_command(value.clone()).await
                                    }
                                    _ => Ok(args[2].clone()),
                                }
                            }
                        }
                        _ => Err(command_type.gen_err(ERROR_ARG_ONE_MUST_BE_BOOL)),
                    }
                }
            }
            CommandType::Not => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    let arg = &args[0].get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Bool(bool) => Ok(ValueType::Bool(!*bool)),
                        _ => Err(command_type.gen_err(ERROR_ARG_ONE_MUST_BE_BOOL)),
                    }
                }
            }
            CommandType::And => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                }
                let mut value: bool = true;
                for arg in &args {
                    let arg = arg.get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Bool(bool) => value = value & bool,
                        _ => return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_BOOL)),
                    }
                }
                Ok(ValueType::Bool(value))
            }
            CommandType::Or => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                }
                let mut value: bool = false;
                for arg in &args {
                    let arg = arg.get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Bool(bool) => value = value | bool,
                        _ => return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_BOOL)),
                    }
                }
                Ok(ValueType::Bool(value))
            }
            CommandType::Eq => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Text(value_a), ValueType::Text(value_b)) => {
                            Ok(ValueType::Bool(&value_a[..] == &value_b[..]))
                        }
                        (ValueType::Int(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a == *value_b))
                        }
                        (ValueType::Int(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool((*value_a as f64) == *value_b))
                        }
                        (ValueType::Float(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a == (*value_b as f64)))
                        }
                        (ValueType::Float(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool(
                                *value_a < *value_b + 0.0001 && *value_a > *value_b - 0.0001,
                            ))
                        }
                        (ValueType::Bool(value_a), ValueType::Bool(value_b)) => {
                            Ok(ValueType::Bool(*value_a == *value_b))
                        }
                        _ => Err(command_type.gen_err(&format!(
                            "Cannot compare {} with {}",
                            args[0].to_string(&self.vars)?,
                            args[1].to_string(&self.vars)?
                        ))),
                    }
                }
            }
            CommandType::Gt => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Int(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a > *value_b))
                        }
                        (ValueType::Int(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool((*value_a as f64) > *value_b))
                        }
                        (ValueType::Float(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a > (*value_b as f64)))
                        }
                        (ValueType::Float(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool(*value_a > *value_b))
                        }
                        _ => Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER)),
                    }
                }
            }
            CommandType::Lt => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Int(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a < *value_b))
                        }
                        (ValueType::Int(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool((*value_a as f64) < *value_b))
                        }
                        (ValueType::Float(value_a), ValueType::Int(value_b)) => {
                            Ok(ValueType::Bool(*value_a < (*value_b as f64)))
                        }
                        (ValueType::Float(value_a), ValueType::Float(value_b)) => {
                            Ok(ValueType::Bool(*value_a < *value_b))
                        }
                        _ => Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER)),
                    }
                }
            }
            CommandType::StartsWith => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Text(value_a), ValueType::Text(value_b)) => {
                            Ok(ValueType::Bool(value_a.starts_with(value_b)))
                        }
                        _ => Err(command_type.gen_err(ERROR_ARGS_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::EndsWith => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Text(value_a), ValueType::Text(value_b)) => {
                            Ok(ValueType::Bool(value_a.ends_with(value_b)))
                        }
                        _ => Err(command_type.gen_err(ERROR_ARGS_MUST_BE_TEXT)),
                    }
                }
            }
            CommandType::NewLine => {
                if args.len() != 0 {
                    Err(command_type.gen_err(ERROR_NO_ARGS))
                } else {
                    Ok(ValueType::Text("\n".to_string()))
                }
            }
            CommandType::Mod => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else if args.contains_float(&self.vars) {
                    if let Some(mut sum) = args[0].extract_float(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_float(&self.vars) {
                                Some(value) => sum %= value,
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Float(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                } else {
                    if let Some(mut sum) = args[0].extract_int(&self.vars) {
                        for arg in &args[1..args.len()] {
                            match arg.extract_int(&self.vars) {
                                Some(value) => {
                                    if value == 0 {
                                        return Err(command_type.gen_err(ERROR_ZERO_DIVISION));
                                    } else {
                                        sum %= value
                                    }
                                }
                                None => {
                                    return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                                }
                            }
                        }

                        Ok(ValueType::Int(sum))
                    } else {
                        return Err(command_type.gen_err(ERROR_ARGS_MUST_BE_NUMBER));
                    }
                }
            }
            // TODO fix identifier
            CommandType::While => {
                if args.len() < 2 {
                    return Err(command_type.gen_err(ERROR_TWO_OR_MORE_ARGS));
                } else {
                    let mut loop_count: u16 = 0;

                    loop {
                        match &args[0] {
                            ValueType::Command(command) => {
                                match self.eval_command(command.clone()).await? {
                                    ValueType::Bool(value) => {
                                        if value {
                                            for arg in &args[1..args.len()] {
                                                if let ValueType::Command(command) = arg {
                                                    self.eval_command(command.clone()).await?;
                                                } else {
                                                    return Err(command_type.gen_err(
                                                        ERROR_ARGS_AFTER_ARG_ONE_MUST_BE_COMMAND,
                                                    ));
                                                };
                                            }
                                        } else {
                                            return Ok(ValueType::None);
                                        }
                                    }
                                    _ => {
                                        return Err(command_type
                                            .gen_err(ERROR_ARG_ONE_MUST_BE_COMMAND_BOOL));
                                    }
                                }
                            }
                            _ => {
                                return Err(
                                    command_type.gen_err(ERROR_ARG_ONE_MUST_BE_COMMAND_BOOL)
                                );
                            }
                        };

                        loop_count = loop_count.saturating_add(1);

                        if loop_count >= LOOP_LIMIT {
                            return Err(command_type.gen_err("Loop limit exceeded"));
                        }
                    }
                }
            }
            CommandType::Index => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match arg_1 {
                        ValueType::Int(i) => match arg_2 {
                            ValueType::Text(value) => match value.chars().nth(*i as usize) {
                                Some(c) => return Ok(ValueType::Text(c.to_string())),
                                None => return Err(command_type.gen_err("index out of bounds")),
                            },
                            ValueType::List(value) => match value.get(*i as usize) {
                                Some(value_type) => Ok(value_type.clone()),
                                None => return Err(command_type.gen_err("index out of bounds")),
                            },
                            _ => {
                                return Err(command_type
                                    .gen_err("second argument must be of type Text or List"));
                            }
                        },
                        _ => {
                            return Err(
                                command_type.gen_err("first argument must be of type Integer")
                            );
                        }
                    }
                }
            }
            // TODO fix identifier
            CommandType::Slice => {
                if args.len() != 3 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_THREE_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    let arg_3 = &args[2].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Int(a), ValueType::Int(b)) => {
                            let a = *a as usize;
                            let b = *b as usize;

                            match arg_3 {
                                ValueType::Text(value) => {
                                    if a >= b {
                                        return Err(command_type.gen_err(
                                            "first argument must be less than second argument",
                                        ));
                                    } else if a > value.len() || b > value.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        return Ok(ValueType::Text(value[a..b].to_string()));
                                    }
                                }
                                ValueType::List(values) => {
                                    if a >= b {
                                        return Err(command_type.gen_err(
                                            "first argument must be less than second argument",
                                        ));
                                    } else if a > values.len() || b > values.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        return Ok(ValueType::List(values[a..b].to_vec()));
                                    }
                                }
                                _ => {
                                    return Err(command_type
                                        .gen_err("third argument must be of type Text or List"));
                                }
                            }
                        }
                        _ => {
                            return Err(
                                command_type.gen_err("first two arguments must be of type Integer")
                            );
                        }
                    }
                }
            }
            // TODO fix identifier
            CommandType::Length => {
                if args.len() != 1 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_ONE_ARG));
                } else {
                    let arg = &args[0].get_inner_value(&self.vars)?;
                    match arg {
                        ValueType::Text(value) => Ok(ValueType::Int(value.len() as i64)),
                        ValueType::List(values) => Ok(ValueType::Int(values.len() as i64)),
                        _ => {
                            return Err(
                                command_type.gen_err("first argument must be of type Text or List")
                            );
                        }
                    }
                }
            }
            CommandType::Swap => {
                if args.len() != 3 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_THREE_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    let arg_3 = &args[2].get_inner_value(&self.vars)?;
                    match (arg_1, arg_2) {
                        (ValueType::Int(a), ValueType::Int(b)) => {
                            let a = *a as usize;
                            let b = *b as usize;

                            match arg_3 {
                                ValueType::Text(value) => {
                                    if a > value.len() || b > value.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        let mut chars: Vec<_> = value.chars().collect();
                                        chars.swap(a, b);
                                        return Ok(ValueType::Text(chars.into_iter().collect()));
                                    }
                                }
                                ValueType::List(values) => {
                                    if a > values.len() || b > values.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        let mut values = values.clone();
                                        values.swap(a, b);
                                        return Ok(ValueType::List(values));
                                    }
                                }
                                _ => {
                                    return Err(command_type
                                        .gen_err("third argument must be of type Text or List"));
                                }
                            }
                        }
                        _ => {
                            return Err(
                                command_type.gen_err("first two arguments must be of type Integer")
                            );
                        }
                    }
                }
            }
            CommandType::Insert => {
                if args.len() != 3 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_THREE_ARGS));
                } else {
                    let arg_1 = args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    let arg_3 = &args[2].get_inner_value(&self.vars)?;
                    match arg_2 {
                        ValueType::Int(i) => {
                            let i = *i as usize;

                            match arg_3 {
                                ValueType::Text(value) => match arg_1 {
                                    ValueType::Text(text) => {
                                        if i > value.len() {
                                            return Err(command_type.gen_err("index out of bounds"));
                                        } else {
                                            let mut value = value.to_string();
                                            value.insert_str(i, text);
                                            return Ok(ValueType::Text(value));
                                        }
                                    },
                                    _ => Err(command_type.gen_err("first argument must be of type Text when inserting into type Text")),
                                },
                                ValueType::List(values) => match arg_1 {
                                    ValueType::Identifier(_) => Err(command_type.gen_err("cannot insert values of type Identifier into type List")),
                                    ValueType::None => Err(command_type.gen_err("cannot insert values of type None into type List")),
                                    _ => {
                                        if i > values.len() {
                                            return Err(command_type.gen_err("index out of bounds"));
                                        } else {
                                            let mut values = values.to_vec();
                                            values.insert(i, arg_1.clone());
                                            return Ok(ValueType::List(values));
                                        }
                                    }
                                },
                                _ => Err(command_type
                                    .gen_err("third argument must be of type Text or List")),
                            }
                        }
                        _ => Err(command_type.gen_err("second argument must be of type Integer")),
                    }
                }
            }
            CommandType::Remove => {
                if args.len() != 2 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_TWO_ARGS));
                } else {
                    let arg_1 = &args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    match arg_1 {
                        ValueType::Int(i) => {
                            let i = *i as usize;

                            match arg_2 {
                                ValueType::Text(value) => {
                                    if i >= value.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        let mut value = value.to_string();
                                        value.remove(i);
                                        return Ok(ValueType::Text(value));
                                    }
                                }
                                ValueType::List(values) => {
                                    if i >= values.len() {
                                        return Err(command_type.gen_err("index out of bounds"));
                                    } else {
                                        let mut values = values.to_vec();
                                        values.remove(i);
                                        return Ok(ValueType::List(values));
                                    }
                                }
                                _ => Err(command_type
                                    .gen_err("second argument must be of type Text or type List")),
                            }
                        }
                        _ => Err(command_type.gen_err("first argument must be of type Integer")),
                    }
                }
            }
            // TODO fix identifier
            CommandType::Replace => {
                if args.len() != 3 {
                    return Err(command_type.gen_err(ERROR_EXACTLY_THREE_ARGS));
                } else {
                    let arg_1 = args[0].get_inner_value(&self.vars)?;
                    let arg_2 = &args[1].get_inner_value(&self.vars)?;
                    let arg_3 = &args[2].get_inner_value(&self.vars)?;
                    match arg_2 {
                        ValueType::Int(i) => {
                            let i = *i as usize;

                            match arg_3 {
                                ValueType::Text(value) => match arg_1 {
                                    ValueType::Text(text) => {
                                        if i >= value.len() {
                                            return Err(command_type.gen_err("index out of bounds"));
                                        } else {
                                            return Ok(ValueType::Text(value[0..i].to_string() + text + &value[i+1..value.len()]));
                                        }
                                    },
                                    _ => Err(command_type.gen_err("first argument must be of type Text when inserting into type Text")),
                                },
                                ValueType::List(values) => match arg_1 {
                                    ValueType::Identifier(_) => Err(command_type.gen_err("cannot insert values of type Identifier into type List")),
                                    ValueType::None => Err(command_type.gen_err("cannot insert values of type None into type List")),
                                    _ => {
                                        if i >= values.len() {
                                            return Err(command_type.gen_err("index out of bounds"));
                                        } else {
                                            let mut values = values.to_vec();
                                            values[i] = arg_1.clone();
                                            return Ok(ValueType::List(values));
                                        }
                                    }
                                },
                                _ => Err(command_type
                                    .gen_err("third argument must be of type Text or List")),
                            }
                        }
                        _ => Err(command_type.gen_err("second argument must be of type Integer")),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::interpreter::documentation::*;

    #[tokio::test]
    async fn validate_documentation_examples() {
        let mut interpreter = Interpreter::new();
        let documentation = get_command_documentation();
        for command in &documentation {
            for example in &command.examples {
                if let Some((code, expected_output)) = example.split_once("=") {
                    let output = interpreter
                        .interpret_embedded_code(code)
                        .await
                        .unwrap()
                        .trim()
                        .to_owned();
                    let expected_output = expected_output.trim();
                    if output != expected_output {
                        eprintln!("code: {}\noutput: {}\n", code, output);
                    }
                    assert_eq!(output, expected_output);
                }
            }
        }
    }

    #[tokio::test]
    async fn validate_random_range_command() {
        let mut interpreter = Interpreter::new();
        let output = interpreter
            .interpret("print(random_range(1,10))")
            .await
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert!(output >= 1 && output <= 10);
    }

    #[tokio::test]
    async fn validate_select_random_command() {
        let mut interpreter = Interpreter::new();
        let output = interpreter
            .interpret("print(select_random(\"a\", \"b\", \"c\", 1, 2, 3))")
            .await
            .unwrap();
        let possible_outputs = ["a", "b", "c", "1", "2", "3"];
        assert!(possible_outputs.contains(&&output[..]));
    }
}
