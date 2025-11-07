use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
};

use ollama_rs::{generation::completion::GenerationResponse, models::ModelInfo};
use rand::{Rng, distr::uniform::SampleUniform};
use regex::Regex;

use crate::{
    interpreter::Interpreter,
    ollama::{OllamaGenerator, OllamaSettings},
    template_database::{
        KeySize, Limit, OrderBy, Substitute, SubstituteReceipt, Template, TemplateDatabase,
        TemplateReceipt,
    },
    template_substitutor::{TemplateDelimiter, TemplateSubstitutor, VALID_TEMPLATE_CHARS},
};

pub mod interpreter;
pub mod ollama;
pub mod template_database;
pub mod template_substitutor;

#[derive(Debug, Clone)]
pub enum FunboyError {
    Interpreter(String),
    Ollama(String),
    Database(String),
    UserInput(String),
}

impl ToString for FunboyError {
    fn to_string(&self) -> String {
        match self {
            FunboyError::Interpreter(e) => {
                format!("FSL interpreter error:\n{}", e)
            }
            FunboyError::Ollama(e) => {
                format!("Ollama error:\n{}", e)
            }
            FunboyError::Database(e) => {
                format!("Database error:\n{}", e)
            }
            FunboyError::UserInput(e) => {
                format!("User input error:\n{}", e)
            }
        }
    }
}

impl Into<FunboyError> for sqlx::Error {
    fn into(self) -> FunboyError {
        FunboyError::Database(self.to_string())
    }
}

pub struct Funboy {
    template_db: TemplateDatabase,
    ollama_generator: OllamaGenerator,
    valid_template_regex: Regex,
}

impl Funboy {
    pub fn new(template_db: TemplateDatabase) -> Self {
        Self {
            template_db,
            ollama_generator: OllamaGenerator::default(),
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
        inclusive: bool,
    ) -> Result<String, &'static str> {
        match (min.parse(), max.parse()) {
            (Ok(min), Ok(max)) => {
                if min >= max {
                    Err("min must be less than max")
                } else {
                    if inclusive {
                        Ok(Self::gen_rand_num_inclusive::<T>(min, max).to_string())
                    } else {
                        Ok(Self::gen_rand_num_exclusive::<T>(min, max).to_string())
                    }
                }
            }
            _ => Err("min and max values must be a number"),
        }
    }

    pub fn random_number(min: &str, max: &str, inclusive: bool) -> Result<String, FunboyError> {
        if min.contains('.') || max.contains('.') {
            match Self::gen_rand_num_from_str::<f64>(min, max, inclusive) {
                Ok(result) => Ok(result),

                Err(e) => Err(FunboyError::UserInput(e.to_string())),
            }
        } else {
            match Self::gen_rand_num_from_str::<i64>(min, max, inclusive) {
                Ok(result) => Ok(result),

                Err(e) => Err(FunboyError::UserInput(e.to_string())),
            }
        }
    }

    // Previously "random_word"
    pub fn random_entry<'b>(list: &[&'b str]) -> Result<&'b str, FunboyError> {
        if list.len() < 2 {
            Err(FunboyError::UserInput(
                "list must contain at least two entries".to_string(),
            ))
        } else {
            let output = list[Self::gen_rand_num_inclusive(0, list.len() - 1)];
            Ok(output)
        }
    }

    pub const MAX_TEMPLATE_LENGTH: usize = 255;
    fn validate_template_name(&self, template: &str) -> Result<(), FunboyError> {
        if !self.valid_template_regex.is_match(template) {
            return Err(FunboyError::UserInput(
                "template must be lowercase containing only characters a-z, 0-9, and _".to_string(),
            ));
        } else if template.len() > Funboy::MAX_TEMPLATE_LENGTH {
            return Err(FunboyError::UserInput(
                "template must be less than 255 characters long".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn add_substitutes<'a>(
        &self,
        template: &str,
        substitutes: &[&'a str],
    ) -> Result<SubstituteReceipt, FunboyError> {
        self.validate_template_name(template)?;

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
    ) -> Result<SubstituteReceipt, FunboyError> {
        self.validate_template_name(template)?;

        match self
            .template_db
            .delete_substitutes_by_name(template, substitutes)
            .await
        {
            Ok(sub_record) => Ok(sub_record),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_substitutes_by_id(
        &self,
        ids: &[KeySize],
    ) -> Result<SubstituteReceipt, FunboyError> {
        match self.template_db.delete_substitutes_by_id(ids).await {
            Ok(subs) => Ok(subs),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn copy_substitutes(
        &self,
        from_template: &str,
        to_template: &str,
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template_name(from_template)?;
        self.validate_template_name(to_template)?;

        match self
            .template_db
            .copy_substitutes_from_template_to_template(from_template, to_template)
            .await
        {
            Ok(subs) => Ok(subs),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn replace_substitute(
        &self,
        template: &str,
        old: &str,
        new: &str,
    ) -> Result<Option<Substitute>, FunboyError> {
        self.validate_template_name(template)?;

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
    ) -> Result<Option<Substitute>, FunboyError> {
        match self.template_db.update_substitute_by_id(id, new).await {
            Ok(sub) => Ok(sub),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_template(&self, template: &str) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(template)?;

        match self.template_db.delete_template_by_name(template).await {
            Ok(template) => Ok(template),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete_templates(
        &self,
        templates: &[&str],
    ) -> Result<TemplateReceipt, FunboyError> {
        for template in templates {
            self.validate_template_name(template)?;
        }

        match self.template_db.delete_templates_by_name(templates).await {
            Ok(template) => Ok(template),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn rename_template(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(from)?;
        self.validate_template_name(to)?;

        match self.template_db.update_template_by_name(from, to).await {
            Ok(template) => Ok(template),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_templates(
        &self,
        search_term: Option<&str>,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Template>, FunboyError> {
        match self
            .template_db
            .read_templates(search_term, order, limit)
            .await
        {
            Ok(templates) => Ok(templates),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_substitutes(
        &self,
        template: &str,
        search_term: Option<&str>,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template_name(template)?;

        match self
            .template_db
            .read_substitutes_from_template(template, search_term, order, limit)
            .await
        {
            Ok(substitutes) => Ok(substitutes),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_random_substitute(&self, template: &str) -> Result<Substitute, FunboyError> {
        self.validate_template_name(template)?;

        match self
            .get_substitutes(template, None, OrderBy::Random, Limit::Count(1))
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

    /// Resolves templates and interprets embeded code in input with a single pass
    async fn interpret_input(
        &self,
        input: String,
        template_substitutors: Vec<TemplateSubstitutor>,
    ) -> Result<String, FunboyError> {
        let mut substituted_text = input.clone();
        for template_substitutor in template_substitutors {
            substituted_text = template_substitutor
                .substitute_recursively(substituted_text, |template: String| async move {
                    match self.get_random_substitute(&template).await {
                        Ok(sub) => Some(sub.name.to_string()),
                        Err(_) => None,
                    }
                })
                .await;
        }

        let mut fsl_interpreter = Interpreter::new();
        let interpreter_result = fsl_interpreter
            .interpret_embedded_code(&substituted_text)
            .await;

        match interpreter_result {
            Ok(interpreted_text) => Ok(interpreted_text),
            Err(e) => Err(FunboyError::Interpreter(e)),
        }
    }

    /// Resolves templates and fsl code until output is complete or depth limit is reached
    pub async fn generate(&self, input: &str) -> Result<String, FunboyError> {
        let mut output = input.to_string();
        let mut prev_hashes = HashSet::new();

        const MAX_GENERATIONS: u8 = 255;
        for _ in 0..MAX_GENERATIONS {
            let mut hasher = DefaultHasher::new();
            output.hash(&mut hasher);
            let hash = hasher.finish();

            if !prev_hashes.insert(hash) {
                break;
            } else {
                output = self
                    .interpret_input(
                        output,
                        vec![
                            TemplateSubstitutor::new(TemplateDelimiter::Caret),
                            TemplateSubstitutor::new(TemplateDelimiter::SingleQuote),
                        ],
                    )
                    .await?;

                output = self
                    .interpret_input(
                        output,
                        vec![TemplateSubstitutor::new(TemplateDelimiter::BackTick)],
                    )
                    .await?;
            }
        }

        Ok(output)
    }

    pub async fn get_ollama_models(&self) -> Result<Vec<String>, FunboyError> {
        let models = self.ollama_generator.get_models().await;
        match models {
            Ok(models) => Ok(models.iter().map(|m| m.name.to_string()).collect()),
            Err(e) => Err(FunboyError::Ollama(e.to_string())),
        }
    }

    pub async fn get_ollama_model_info(&self, model: String) -> Result<ModelInfo, FunboyError> {
        match self.ollama_generator.get_model_info(model).await {
            Ok(info) => Ok(info),
            Err(e) => Err(FunboyError::Ollama(e.to_string())),
        }
    }

    pub async fn generate_ollama(
        &self,
        model: Option<String>,
        ollama_settings: &OllamaSettings,
        prompt: &str,
    ) -> Result<GenerationResponse, FunboyError> {
        let prompt = self.generate(prompt).await?;
        match self
            .ollama_generator
            .generate(&prompt, ollama_settings, model)
            .await
        {
            Ok(output) => Ok(output),
            Err(e) => Err(FunboyError::Ollama(e.to_string())),
        }
    }
}

#[cfg(test)]
mod core {
    use super::*;
    use sqlx::PgPool;
    use std::{panic, sync::Arc};
    use template_database::test::create_debug_db;

    #[tokio::test]
    async fn random_number_produces_int_in_range() {
        for _ in 0..100 {
            let result = Funboy::random_number("1", "6", true)
                .unwrap()
                .parse::<i64>()
                .unwrap();
            assert!((1..=6).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_produces_float() {
        for _ in 0..100 {
            let result = Funboy::random_number("1.0", "6.0", true)
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((1.0..=6.0).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_fails_when_min_greater_than_max() {
        match Funboy::random_number("6", "1", true) {
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
        match Funboy::random_number("6", "6", true) {
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
        let result = Funboy::random_entry(&["one", "two", "three", "four"]).unwrap();

        if !(&["one", "two", "three", "four"].contains(&result)) {
            panic!("array should contain result");
        }
    }

    #[tokio::test]
    async fn random_entry_fails_with_less_than_two_entries() {
        match Funboy::random_entry(&["one"]) {
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

    async fn get_pool() -> PgPool {
        PgPool::connect(template_database::DEBUG_DB_URL)
            .await
            .unwrap()
    }

    async fn get_funboy(pool: PgPool) -> Funboy {
        let db = create_debug_db(pool).await.unwrap();
        Funboy::new(db)
    }

    #[tokio::test]
    async fn generate_templates() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

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
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        let output = funboy
            .generate("{repeat(5, print(\"again\"))}")
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "againagainagainagainagain");
    }

    #[tokio::test]
    async fn generate_lazy_templates() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

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
    async fn generate_lazy_templates_that_contain_code() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        funboy.add_substitutes("adj", &["quick"]).await.unwrap();
        funboy.add_substitutes("color", &["brown"]).await.unwrap();
        funboy.add_substitutes("noun", &["fox"]).await.unwrap();
        funboy.add_substitutes("verb", &["jump"]).await.unwrap();
        funboy
            .add_substitutes(
                "quick_brown_fox",
                &["{print(\"The ^adj ^color ^noun ^verb^ed over the lazy dog.\")}"],
            )
            .await
            .unwrap();

        let output = funboy
            .generate("{print(\"`quick_brown_fox`\")}")
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "The quick brown fox jumped over the lazy dog.");
    }

    #[tokio::test]
    async fn validate_template_names() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

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
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn generate_ollama_response() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        funboy
            .add_substitutes("adj", &["funny", "evil", "small", "big"])
            .await
            .unwrap();

        let generation_response = funboy
            .generate_ollama(
                Some("tinyllama".to_string()),
                &OllamaSettings::default(),
                "{print(\"You are very ^adj you know that?\")}",
            )
            .await
            .unwrap();

        println!("Ollama response: {}", generation_response.response);
    }
}
