use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use async_recursion::async_recursion;
use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::TEXT_TYPES,
    types::{
        command::{ArgPos, ArgRule, Command, CommandError, Executor},
        value::Value,
    },
};
use moka::future::{Cache, CacheBuilder};
use ollama_rs::{generation::completion::GenerationResponse, models::ModelInfo};
use rand::{Rng, distr::uniform::SampleUniform, random_range};
use regex::Regex;
use tokio::sync::Mutex;

use crate::{
    ollama::{OllamaGenerator, OllamaSettings},
    template_database::{
        KeySize, Limit, OrderBy, Substitute, SubstituteReceipt, Template, TemplateDatabase,
        TemplateReceipt,
    },
    template_substitutor::{TemplateDelimiter, TemplateSubstitutor, VALID_TEMPLATE_CHARS},
};

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

impl From<sqlx::Error> for FunboyError {
    fn from(value: sqlx::Error) -> Self {
        eprintln!("{}", value);
        FunboyError::Database(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Funboy {
    template_db: TemplateDatabase,
    ollama_generator: OllamaGenerator,
    valid_template_regex: Regex,
    random_sub_cache: Arc<Cache<String, Vec<Substitute>>>,
}

impl Funboy {
    pub fn new(template_db: TemplateDatabase) -> Self {
        Self {
            template_db,
            ollama_generator: OllamaGenerator::default(),
            valid_template_regex: Regex::new(&format!("^[{}]+$", VALID_TEMPLATE_CHARS)).unwrap(),
            random_sub_cache: Arc::new(
                CacheBuilder::new(20)
                    .time_to_live(Duration::from_secs(60))
                    .build(),
            ),
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
        if template.is_empty() {
            return Err(FunboyError::UserInput(
                "template cannot be empty".to_string(),
            ));
        } else if template.chars().nth(0).is_some_and(|ch| ch.is_numeric()) {
            return Err(FunboyError::UserInput(
                "first character of template cannot be a number".to_string(),
            ));
        } else if !self.valid_template_regex.is_match(template) {
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

        let receipt = self.template_db.create_substitutes(template, substitutes);
        let receipt = receipt.await?;
        self.random_sub_cache.invalidate(template).await;
        Ok(receipt)
    }

    pub async fn delete_substitutes<'a>(
        &self,
        template: &str,
        substitutes: &[&'a str],
    ) -> Result<SubstituteReceipt, FunboyError> {
        self.validate_template_name(template)?;

        let receipt = self
            .template_db
            .delete_substitutes_by_name(template, substitutes);
        let receipt = receipt.await?;
        self.random_sub_cache.invalidate(template).await;
        Ok(receipt)
    }

    pub async fn delete_substitutes_by_id(
        &self,
        ids: &[KeySize],
    ) -> Result<SubstituteReceipt, FunboyError> {
        let receipt = self.template_db.delete_substitutes_by_id(ids);
        let receipt = receipt.await?;
        for sub in &receipt.updated {
            let template = self.template_db.read_template_by_id(sub.template_id);
            let template = template.await?.expect("sub must be inside template");
            self.random_sub_cache.invalidate(&template.name).await;
        }
        Ok(receipt)
    }

    pub async fn copy_substitutes(
        &self,
        from_template: &str,
        to_template: &str,
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template_name(from_template)?;
        self.validate_template_name(to_template)?;

        let subs = self
            .template_db
            .copy_substitutes_from_template_to_template(from_template, to_template);
        let subs = subs.await?;
        self.random_sub_cache.invalidate(to_template).await;
        Ok(subs)
    }

    pub async fn replace_substitute(
        &self,
        template: &str,
        old: &str,
        new: &str,
    ) -> Result<Option<Substitute>, FunboyError> {
        self.validate_template_name(template)?;

        let sub = self
            .template_db
            .update_substitute_by_name(template, old, new);
        let sub = sub.await?;
        self.random_sub_cache.invalidate(template).await;
        Ok(sub)
    }

    pub async fn replace_substitute_by_id(
        &self,
        id: KeySize,
        new: &str,
    ) -> Result<Option<Substitute>, FunboyError> {
        let sub = self.template_db.update_substitute_by_id(id, new);
        let sub = sub.await?;
        if let Some(sub) = sub.as_ref() {
            let template = self.template_db.read_template_by_id(sub.template_id);
            let template = template.await?.expect("sub must be inside template");
            self.random_sub_cache.invalidate(&template.name).await;
        }
        Ok(sub)
    }

    pub async fn delete_template(&self, template: &str) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(template)?;

        let template = self.template_db.delete_template_by_name(template);
        let template = template.await?;
        if let Some(template) = template.as_ref() {
            self.random_sub_cache.invalidate(&template.name).await;
        }
        Ok(template)
    }

    pub async fn delete_templates(
        &self,
        templates: &[&str],
    ) -> Result<TemplateReceipt, FunboyError> {
        for template in templates {
            self.validate_template_name(template)?;
        }

        let receipt = self.template_db.delete_templates_by_name(templates);
        let receipt = receipt.await?;
        for template in &receipt.updated {
            self.random_sub_cache.invalidate(&template.name).await;
        }
        Ok(receipt)
    }

    pub async fn rename_template(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(from)?;
        self.validate_template_name(to)?;

        let template = self.template_db.update_template_by_name(from, to);
        let template = template.await?;
        self.random_sub_cache.invalidate(from).await;
        Ok(template)
    }

    pub async fn get_templates(
        &self,
        search_term: Option<&str>,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Template>, FunboyError> {
        let templates = self.template_db.read_templates(search_term, order, limit);
        let templates = templates.await?;
        Ok(templates)
    }

    pub async fn get_substitutes(
        &self,
        template: &str,
        search_term: Option<&str>,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Substitute>, FunboyError> {
        self.validate_template_name(template)?;
        let subs =
            self.template_db
                .read_substitutes_from_template(template, search_term, order, limit);
        let subs = subs.await?;
        Ok(subs)
    }

    async fn get_random_substitute(&self, template: &str) -> Result<Substitute, FunboyError> {
        self.validate_template_name(template)?;

        match self.random_sub_cache.get(template).await {
            Some(subs) => {
                let sub = subs
                    .get(random_range(0..subs.len()))
                    .expect("subs should be present in cache if match was found");
                Ok(sub.clone())
            }
            None => {
                let subs = self.get_substitutes(template, None, OrderBy::Random, Limit::Count(200));
                let subs = subs.await?;

                if !subs.is_empty() {
                    let rnd_range = random_range(0..subs.len());
                    let sub = subs
                        .get(rnd_range)
                        .cloned()
                        .expect("subs cannot be empty due to explicit check");
                    self.random_sub_cache
                        .insert(template.to_string(), subs)
                        .await;
                    Ok(sub)
                } else {
                    Err(FunboyError::Database(format!(
                        "No substitutes were present in template \"{}\"",
                        template
                    )))
                }
            }
        }
    }

    /// Resolves templates and interprets embeded code in input with a single pass
    async fn interpret_input(
        &self,
        input: String,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let mut substituted_text = self
            .substitute_register_templates(input, interpreter.clone())
            .await?;

        substituted_text = TemplateSubstitutor::new(TemplateDelimiter::Caret)
            .substitute_recursively(substituted_text, |template: String| async move {
                match self.get_random_substitute(&template).await {
                    Ok(sub) => Some(sub.name.to_string()),
                    Err(_) => None,
                }
            })
            .await;

        let mut interpreter = interpreter.lock().await;
        let interpreter_result = interpreter.interpret_embedded_code(&substituted_text).await;

        match interpreter_result {
            Ok(interpreted_text) => Ok(interpreted_text),
            Err(e) => Err(FunboyError::Interpreter(e.to_string())),
        }
    }

    #[async_recursion]
    async fn substitute_register_templates(
        &self,
        input: String,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let sub_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let funboy_error: Arc<Mutex<Option<FunboyError>>> = Arc::new(Mutex::new(None));
        let output = TemplateSubstitutor::new(TemplateDelimiter::PlusRegister)
            .substitute_recursively(input, |template: String| {
                let sub_map = sub_map.clone();
                let interpreter = interpreter.clone();
                let funboy_error = funboy_error.clone();

                async move {
                    let mut sub_map = sub_map.lock().await;
                    let result = sub_map.get(&template);
                    if let Some(value) = result {
                        Some(value.clone())
                    } else {
                        let split = template.split('-').collect::<Vec<&str>>();
                        let template_before_dash = split.get(0).unwrap_or(&"");
                        match self.get_random_substitute(&template_before_dash).await {
                            Ok(sub) => {
                                let sub = match self.generate(&sub.name, interpreter).await {
                                    Ok(interpreted_sub) => interpreted_sub,
                                    Err(e) => {
                                        let _ = funboy_error.lock().await.insert(e);
                                        return None;
                                    }
                                };
                                sub_map.insert(template.to_string(), sub.clone());
                                return Some(sub);
                            }
                            Err(_) => None,
                        }
                    }
                }
            })
            .await;
        let err = funboy_error.lock().await.take();
        match err {
            Some(e) => return Err(e),
            None => return Ok(output),
        }
    }

    /* PROFILE CODE
        let before = SystemTime::now();

        let after = SystemTime::now();

        let time = after.duration_since(before).unwrap();
        unsafe {
            static mut INTERP_TIME: Duration = Duration::new(0, 0);
            INTERP_TIME += time;
            dbg!(INTERP_TIME);
        }
    */
    /// Resolves templates and fsl code until output is complete or depth limit is reached
    pub async fn generate(
        &self,
        input: &str,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let mut output = input.to_string();
        let mut prev_hashes = HashSet::new();

        let mut modified_interpreter = interpreter.lock().await;
        let funboy = Arc::new(self.clone());
        modified_interpreter.add_command(GET_SUB, GET_SUB_RULES, create_get_sub_command(funboy));
        drop(modified_interpreter);

        const MAX_GENERATIONS: u8 = 255;
        for _ in 0..MAX_GENERATIONS {
            let mut hasher = DefaultHasher::new();
            output.hash(&mut hasher);
            let hash = hasher.finish();

            if !prev_hashes.insert(hash) {
                break;
            } else {
                output = self.interpret_input(output, interpreter.clone()).await?;
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
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<GenerationResponse, FunboyError> {
        let prompt = self.generate(prompt, interpreter).await?;
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

const GET_SUB: &str = "get_sub";
const GET_SUB_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
fn create_get_sub_command(funboy: Arc<Funboy>) -> Executor {
    let get_sub_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let funboy = funboy.clone();
            async move {
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data).await?;
                if template.starts_with('`') {
                    let template = template.trim_matches('`');
                    let sub = funboy.get_random_substitute(template).await;
                    match sub {
                        Ok(sub) => Ok(Value::Text(sub.name)),
                        Err(e) => Err(CommandError::Custom(e.to_string())),
                    }
                } else {
                    return Err(CommandError::Custom(
                        "template name must be preceeded by `".to_string(),
                    ));
                }
            }
        }
    };
    Some(Arc::new(get_sub_command))
}

#[cfg(test)]
mod core {
    use super::*;
    use sqlx::PgPool;
    use std::panic;
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

        let output = funboy
            .generate("^sentence", Arc::new(Mutex::new(FslInterpreter::new())))
            .await
            .unwrap();

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

        let output = funboy
            .generate("^sentence", Arc::new(Mutex::new(FslInterpreter::new())))
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "A quick brown fox jumped over the lazy dog.");
    }

    #[tokio::test]
    async fn generate_copied_template() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        funboy
            .add_substitutes(
                "noun",
                &["fox", "bear", "lion", "tiger", "bat", "giraffe", "zebra"],
            )
            .await
            .unwrap();

        let output = funboy
            .generate(
                "$noun $noun $noun $noun $noun",
                Arc::new(Mutex::new(FslInterpreter::new())),
            )
            .await
            .unwrap();

        let mut subs = output.split_whitespace();
        let first_sub = subs.nth(0).unwrap();
        for sub in subs {
            dbg!(sub);
            assert!(sub == first_sub);
        }
    }

    #[tokio::test]
    async fn generate_copied_template_registers() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        funboy
            .add_substitutes(
                "noun",
                &["fox", "bear", "lion", "tiger", "bat", "giraffe", "zebra"],
            )
            .await
            .unwrap();

        let output = funboy
            .generate(
                "$noun-1 $noun-1 $noun-2 $noun-2 $noun-2 $noun-999 $noun-999 $noun-999$$noun-999$",
                Arc::new(Mutex::new(FslInterpreter::new())),
            )
            .await
            .unwrap();

        // relies on random, can't assert, dbg output
        dbg!(output);
    }

    #[tokio::test]
    async fn generate_code() {
        let pool = get_pool().await;
        let funboy = get_funboy(pool).await;

        let output = funboy
            .generate(
                "{repeat(5, print(\"again\"))}",
                Arc::new(Mutex::new(FslInterpreter::new())),
            )
            .await
            .unwrap();

        println!("OUTPUT: {}", output);
        assert!(output == "againagainagainagainagain");
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

    // Test is slow so only run it selectively
    // #[tokio::test]
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
                Arc::new(Mutex::new(FslInterpreter::new())),
            )
            .await
            .unwrap();

        println!("Ollama response: {}", generation_response.response);
    }
}
