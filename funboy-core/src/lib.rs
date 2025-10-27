use std::str::FromStr;

use rand::{Rng, distributions::uniform::SampleUniform};

pub mod database;
pub mod database_old;
pub mod interpolator;
pub mod interpreter;
pub mod ollama;

#[derive(Debug, Clone)]
pub enum FunboyError {
    Interpolator(String),
    Interpreter(String),
    AI(String),
    Database(String),
    UserInput(String),
}

pub struct Funboy {}

impl Funboy {
    fn gen_rand_num_inclusive<T: SampleUniform + PartialOrd>(min: T, max: T) -> T {
        let mut rng = rand::thread_rng();
        rng.gen_range(min..=max)
    }

    fn gen_rand_num_exclusive<T: SampleUniform + PartialOrd>(min: T, max: T) -> T {
        let mut rng = rand::thread_rng();
        rng.gen_range(min..max)
    }

    fn gen_rand_num_from_str<T: FromStr + PartialOrd + SampleUniform + ToString>(
        min: &str,
        max: &str,
    ) -> Result<String, &'static str> {
        match (min.parse(), max.parse()) {
            (Ok(min), Ok(max)) => {
                if min >= max {
                    Err("min must be less than max")
                } else {
                    Ok(Self::gen_rand_num_inclusive::<T>(min, max).to_string())
                }
            }
            _ => Err("min and max values must be a number"),
        }
    }

    pub async fn random_number(min: &str, max: &str) -> Result<String, FunboyError> {
        if min.contains('.') || max.contains('.') {
            match Self::gen_rand_num_from_str::<f64>(min, max) {
                Ok(result) => Ok(result),

                Err(e) => Err(FunboyError::UserInput(e.to_string())),
            }
        } else {
            match Self::gen_rand_num_from_str::<i64>(min, max) {
                Ok(result) => Ok(result),

                Err(e) => Err(FunboyError::UserInput(e.to_string())),
            }
        }
    }

    // Previously "random_word"
    pub async fn random_entry<'a>(list: &[&'a str]) -> Result<&'a str, FunboyError> {
        if list.len() < 2 {
            Err(FunboyError::UserInput(
                "list must contain at least two entries".to_string(),
            ))
        } else {
            let output = list[Self::gen_rand_num_inclusive(0, list.len() - 1)];
            Ok(output)
        }
    }

    pub async fn add_substitutes(
        template_name: &str,
        substitutes: &[&str],
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn remove_substitutes(
        template_name: &str,
        substitutes: &[&str],
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn remove_substitutes_by_id(
        template_name: &str,
        ids: &[usize],
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn copy_substitutes(
        from_template: &str,
        to_template: &str,
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn replace_substitute(
        template: &str,
        substitute: &str,
        replacement: &str,
    ) -> Result<(), String> {
        todo!()
    }

    pub async fn replace_substitute_by_id(
        template: &str,
        id: usize,
        replacement: &str,
    ) -> Result<(), String> {
        todo!()
    }

    pub async fn remove_template(template: &str) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn rename_template(from: &str, to: &str) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn generate(input: &str) -> Result<String, FunboyError> {
        todo!()
    }

    pub async fn get_templates() -> Result<Vec<String>, FunboyError> {
        todo!()
    }

    pub async fn get_substitutes(template: &str) -> Result<Vec<(usize, String)>, FunboyError> {
        todo!()
    }

    pub async fn get_ai_models() -> Result<Vec<String>, FunboyError> {
        todo!()
    }

    pub async fn set_ai_models(model_name: &str) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn get_ai_settings() -> Result<String, FunboyError> {
        todo!()
    }

    pub async fn set_ai_token_limit(limit: u16) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn set_ai_parameters(
        temperature: Option<f32>,
        repeat_penalty: Option<f32>,
        top_k: Option<u32>,
        top_p: Option<f32>,
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn reset_ai_parameters() {
        todo!()
    }

    pub async fn set_ai_system_prompt(system_prompt: &str) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn reset_ai_system_prompt() -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn set_ai_template(template: &str) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn reset_ai_template() -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn generate_ai(prompt: &str) -> Result<String, FunboyError> {
        todo!()
    }
}

#[cfg(test)]
mod core {
    use super::*;
    use std::panic;

    #[tokio::test]
    async fn random_number_produces_int_in_range() {
        for _ in 0..100 {
            let result = Funboy::random_number("1", "6")
                .await
                .unwrap()
                .parse::<i64>()
                .unwrap();
            assert!((1..=6).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_produces_float() {
        for _ in 0..100 {
            let result = Funboy::random_number("1.0", "6.0")
                .await
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((1.0..=6.0).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_fails_when_min_greater_than_max() {
        match Funboy::random_number("6", "1").await {
            Ok(_) => {
                panic!("Value should not be Ok");
            }
            Err(e) => {
                assert!(
                    matches!(e, FunboyError::UserInput(_)),
                    "error was not UserInput variant"
                );
            }
        }
    }

    #[tokio::test]
    async fn random_number_fails_when_min_equal_to_max() {
        match Funboy::random_number("6", "6").await {
            Ok(_) => {
                panic!("Value should not be Ok");
            }
            Err(e) => {
                assert!(
                    matches!(e, FunboyError::UserInput(_)),
                    "error was not UserInput variant"
                );
            }
        }
    }

    #[tokio::test]
    async fn random_entry_returns_correct_output() {
        let result = Funboy::random_entry(&["one", "two", "three", "four"])
            .await
            .unwrap();

        if !(&["one", "two", "three", "four"].contains(&result)) {
            panic!("array should contain result");
        }
    }

    #[tokio::test]
    async fn random_entry_fails_with_less_than_two_entries() {
        match Funboy::random_entry(&["one"]).await {
            Ok(_) => {
                panic!("Value should not be Ok");
            }
            Err(e) => {
                assert!(
                    matches!(e, FunboyError::UserInput(_)),
                    "error was not UserInput variant"
                );
            }
        }
    }
}
