use std::str::FromStr;

use rand::{Rng, distr::uniform::SampleUniform};
use regex::Regex;

use crate::{
    interpreter::Interpreter,
    template_database::{KeySize, Limit, OrderBy, Substitute, Template, TemplateDatabase},
    template_substitutor::{TemplateDelimiter, TemplateSubstitutor, VALID_TEMPLATE_CHARS},
};

pub mod interpreter;
pub mod ollama;
pub mod template_database;
pub mod template_substitutor;

#[derive(Debug, Clone)]
pub enum FunboyError {
    Interpreter(String),
    AI(String),
    Database(String),
    UserInput(String),
}

impl Into<FunboyError> for sqlx::Error {
    fn into(self) -> FunboyError {
        FunboyError::Database(self.to_string())
    }
}

pub struct Funboy {
    pub template_db: TemplateDatabase,
    valid_template_regex: Regex,
}

impl Funboy {
    pub fn new(template_db: TemplateDatabase) -> Self {
        Self {
            template_db,
            valid_template_regex: Regex::new(&format!("^[{}]+$", VALID_TEMPLATE_CHARS)).unwrap(),
        }
    }

    fn gen_rand_num_inclusive<T: SampleUniform + PartialOrd>(min: T, max: T) -> T {
        let mut rng = rand::rng();
        rng.random_range(min..=max)
    }

    fn gen_rand_num_exclusive<T: SampleUniform + PartialOrd>(min: T, max: T) -> T {
        let mut rng = rand::rng();
        rng.random_range(min..max)
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

    fn validate_template(&self, template: &str) -> Result<(), FunboyError> {
        if !self.valid_template_regex.is_match(template) {
            return Err(FunboyError::UserInput(
                "template must be lowercase containing only characters a-z, 0-9, and _".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn add_substitutes(
        &self,
        template: &str,
        substitutes: &[&str],
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template(template)?;

        match self
            .template_db
            .create_substitutes(template, substitutes)
            .await
        {
            Ok(subs) => Ok(subs),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_substitutes<'a>(
        &self,
        template: &str,
        substitutes: &[&'a str],
    ) -> Result<(), FunboyError> {
        self.validate_template(template)?;

        match self
            .template_db
            .delete_substitutes_by_name(template, substitutes)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_substitutes_by_id<'a>(&self, ids: &[KeySize]) -> Result<(), FunboyError> {
        match self.template_db.delete_substitutes_by_id(ids).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn copy_substitutes(
        &self,
        from_template: &str,
        to_template: &str,
    ) -> Result<(), FunboyError> {
        todo!()
    }

    pub async fn replace_substitute(
        &self,
        template: &str,
        old: &str,
        new: &str,
    ) -> Result<Substitute, FunboyError> {
        self.validate_template(template)?;

        match self
            .template_db
            .update_substitute_by_name(template, old, new)
            .await
        {
            Ok(sub) => Ok(sub),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn replace_substitute_by_id(
        &self,
        id: KeySize,
        new: &str,
    ) -> Result<Substitute, FunboyError> {
        match self.template_db.update_substitute_by_id(id, new).await {
            Ok(sub) => Ok(sub),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_template(&self, template: &str) -> Result<(), FunboyError> {
        self.validate_template(template)?;

        match self.template_db.delete_template_by_name(template).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn rename_template(&self, from: &str, to: &str) -> Result<(), FunboyError> {
        self.validate_template(from)?;
        self.validate_template(to)?;

        match self.template_db.update_template_by_name(from, to).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_templates(
        &self,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Template>, FunboyError> {
        match self.template_db.read_templates(order, limit).await {
            Ok(templates) => Ok(templates),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_substitutes(
        &self,
        template: &str,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template(template)?;

        match self
            .template_db
            .read_substitutes_from_template(template, order, limit)
            .await
        {
            Ok(substitutes) => Ok(substitutes),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_random_substitute(&self, template: &str) -> Result<Substitute, FunboyError> {
        self.validate_template(template)?;

        match self
            .get_substitutes(template, OrderBy::Random, Limit::Count(1))
            .await
        {
            Ok(subs) => match subs.get(0) {
                Some(sub) => Ok(sub.clone()),
                None => Err(FunboyError::Database(format!(
                    "No substitutes were present in template \"{}\"",
                    template
                ))),
            },
            Err(e) => Err(e.into()),
        }
    }

    pub async fn generate(&self, text: &str) -> Result<String, FunboyError> {
        let template_substitutor = TemplateSubstitutor::default();

        let substituted_text = template_substitutor
            .substitute_recursively(text.to_string(), |template: String| async move {
                match self.get_random_substitute(&template).await {
                    Ok(sub) => Some(sub.name.to_string()),
                    Err(_) => None,
                }
            })
            .await;

        let mut fsl_interpreter = Interpreter::new();

        match fsl_interpreter
            .interpret_embedded_code(&substituted_text)
            .await
        {
            Ok(interpreted_text) => {
                let lazy_substitutor = TemplateSubstitutor::new(TemplateDelimiter::BackTick);

                Ok(lazy_substitutor
                    .substitute_recursively(interpreted_text, |template: String| async move {
                        match self.get_random_substitute(&template).await {
                            Ok(sub) => Some(sub.name.to_string()),
                            Err(_) => None,
                        }
                    })
                    .await)
            }
            Err(e) => Err(FunboyError::Interpreter(e)),
        }
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

    async fn get_funboy() -> Funboy {
        let db = TemplateDatabase::new_debug()
            .await
            .expect("Database should exit and connect");

        Funboy::new(db)
    }

    #[tokio::test]
    async fn generate_templates() {
        let funboy = get_funboy().await;

        let output = funboy.generate("^sentence").await.unwrap();

        assert!(output == "^sentence");
        println!("OUTPUT: {}", output);

        funboy
            .add_substitutes(
                "sentence",
                &["A ^adj brown ^noun ^verb^ed over the lazy dog."],
            )
            .await
            .unwrap();

        funboy.add_substitutes("adj", &["quick"]).await.unwrap();
        funboy.add_substitutes("noun", &["fox"]).await.unwrap();
        funboy.add_substitutes("verb", &["jump"]).await.unwrap();

        let output = funboy.generate("^sentence").await.unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "A quick brown fox jumped over the lazy dog.");
    }

    #[tokio::test]
    async fn generate_code() {
        let funboy = get_funboy().await;

        let output = funboy
            .generate("{repeat(5, print(\"again\"))}")
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "againagainagainagainagain");
    }

    #[tokio::test]
    async fn generate_lazy_templates() {
        let funboy = get_funboy().await;

        funboy.add_substitutes("adj", &["quick"]).await.unwrap();
        funboy.add_substitutes("noun", &["fox"]).await.unwrap();
        funboy.add_substitutes("verb", &["jump"]).await.unwrap();

        let output = funboy
            .generate("{copy(\"`adj\", adj) print(concat(paste(adj), paste(adj)))}")
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "quickadj");

        let output = funboy
            .generate("{copy(\"^adj\", adj) print(concat(paste(adj), paste(adj)))}")
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "quickquick");
    }

    #[tokio::test]
    async fn validate_template_names() {
        let funboy = get_funboy().await;

        assert!(funboy.add_substitutes("NoGood", &["blah"]).await.is_err());

        assert!(funboy.add_substitutes("very_good", &["blah"]).await.is_ok());

        assert!(
            funboy
                .rename_template("notReal", "notRealEither")
                .await
                .is_err_and(|e| matches!(e, FunboyError::UserInput(_)))
        );

        assert!(
            funboy
                .rename_template("real", "notRealEither")
                .await
                .is_err_and(|e| matches!(e, FunboyError::UserInput(_)))
        );

        assert!(
            funboy
                .rename_template("real", "totally_real_too")
                .await
                .is_err_and(|e| matches!(e, FunboyError::Database(_)))
        );
    }
}
