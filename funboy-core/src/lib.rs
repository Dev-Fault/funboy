use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use async_recursion::async_recursion;
use fsl_core::FslInterpreter;
use moka::future::{Cache, CacheBuilder};
use ollama_rs::models::ModelInfo;
use rand::{Rng, distr::uniform::SampleUniform, random_range};
use regex::Regex;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    database::{
        FunboyDatabase, KeySize, Limit, OrderBy, Platform, Substitute, SubstituteReceipt, Template,
        TemplateReceipt,
    },
    interpreter::{
        ADD_SUBS, ADD_SUBS_RULES, ASK_AI, ASK_AI_RULES, GET_SUB, GET_SUB_RULES, add_subs_command,
        ask_ai_command, get_sub_command,
    },
    ollama::{OllamaGenerator, OllamaResponse, OllamaSettings},
    permissions::{Permission, PermissionError, Permissions, Role},
    template_substitutor::{TemplateDelimiter, TemplateSubstitutor, VALID_TEMPLATE_CHARS},
    user::{FlagGuard, FunboyUserId, UserMap},
};

pub mod commands;
pub mod database;
pub mod format;
pub mod interpreter;
pub mod ollama;
pub mod permissions;
pub mod rate_limiter;
pub mod template_substitutor;
pub mod user;

#[derive(Debug, Clone)]
pub enum FunboyError {
    Interpreter(String),
    Ollama(String),
    Database(String),
    UserInput(String),
    UsageLimit(String),
    Permission(PermissionError),
    GenerationCancelled,
}

impl Display for FunboyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
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
            FunboyError::UsageLimit(e) => e.clone(),
            FunboyError::Permission(permission_error) => permission_error.to_string(),
            FunboyError::GenerationCancelled => format!("Generation was cancelled"),
        };
        write!(f, "{}", text)
    }
}

impl From<sqlx::Error> for FunboyError {
    fn from(value: sqlx::Error) -> Self {
        eprintln!("{}", value);
        FunboyError::Database(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub enum Request {
    GenerateFile,
    UploadSub(String),
    DeleteTemplate(String),
}

pub const MAX_TEMPLATE_LENGTH: usize = 255;
#[derive(Debug, Clone)]
pub struct Funboy<U: FunboyUserId> {
    pub users: UserMap<U>,
    funboy_db: FunboyDatabase,
    ollama_model: Arc<Mutex<Option<String>>>,
    ollama_generator: OllamaGenerator,
    valid_template_regex: Regex,
    random_sub_cache: Arc<Cache<String, Vec<Substitute>>>,
}

impl<U: FunboyUserId> Funboy<U> {
    pub fn new(funboy_db: FunboyDatabase, platform: Platform) -> Self {
        Self {
            funboy_db: funboy_db.clone(),
            users: UserMap::new(platform, funboy_db),
            ollama_model: Arc::new(Mutex::new(None)),
            ollama_generator: OllamaGenerator::default(),
            valid_template_regex: Regex::new(&format!("^[{}]+$", VALID_TEMPLATE_CHARS)).unwrap(),
            random_sub_cache: Arc::new(
                CacheBuilder::new(20)
                    .time_to_idle(Duration::from_secs(60))
                    .build(),
            ),
        }
    }

    pub async fn get_ollama_model(&self) -> Option<String> {
        let model = self.ollama_model.lock().await.clone();
        match model {
            Some(_) => model,
            None => self.ollama_generator.get_default_model().await,
        }
    }

    pub async fn set_ollama_model(&self, new_model: Option<String>) {
        let mut model = self.ollama_model.lock().await;
        *model = new_model;
    }

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
        } else if template.len() > MAX_TEMPLATE_LENGTH {
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

        let receipt = self.funboy_db.create_substitutes(template, substitutes);
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
            .funboy_db
            .delete_substitutes_by_name(template, substitutes);
        let receipt = receipt.await?;
        self.random_sub_cache.invalidate(template).await;
        Ok(receipt)
    }

    pub async fn delete_substitutes_by_id(
        &self,
        ids: &[KeySize],
    ) -> Result<SubstituteReceipt, FunboyError> {
        let receipt = self.funboy_db.delete_substitutes_by_id(ids);
        let receipt = receipt.await?;
        for sub in &receipt.updated {
            let template = self.funboy_db.read_template_by_id(sub.template_id);
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
            .funboy_db
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

        let sub = self.funboy_db.update_substitute_by_name(template, old, new);
        let sub = sub.await?;
        self.random_sub_cache.invalidate(template).await;
        Ok(sub)
    }

    pub async fn replace_substitute_by_id(
        &self,
        id: KeySize,
        new: &str,
    ) -> Result<Option<Substitute>, FunboyError> {
        let sub = self.funboy_db.update_substitute_by_id(id, new);
        let sub = sub.await?;
        if let Some(sub) = sub.as_ref() {
            let template = self.funboy_db.read_template_by_id(sub.template_id);
            let template = template.await?.expect("sub must be inside template");
            self.random_sub_cache.invalidate(&template.name).await;
        }
        Ok(sub)
    }

    pub async fn delete_template(&self, template: &str) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(template)?;

        let template = self.funboy_db.delete_template_by_name(template);
        let template = template.await?;
        self.random_sub_cache.invalidate_all();
        Ok(template)
    }

    pub async fn delete_templates(
        &self,
        templates: &[&str],
    ) -> Result<TemplateReceipt, FunboyError> {
        for template in templates {
            self.validate_template_name(template)?;
        }

        let receipt = self.funboy_db.delete_templates_by_name(templates);
        let receipt = receipt.await?;
        self.random_sub_cache.invalidate_all();
        Ok(receipt)
    }

    pub async fn rename_template(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Option<Template>, FunboyError> {
        self.validate_template_name(from)?;
        self.validate_template_name(to)?;

        let template = self.funboy_db.update_template_by_name(from, to);
        let template = template.await?;
        self.random_sub_cache.invalidate_all();
        Ok(template)
    }

    pub async fn get_templates(
        &self,
        search_term: Option<&str>,
        order: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Template>, FunboyError> {
        let templates = self.funboy_db.read_templates(search_term, order, limit);
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
            self.funboy_db
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
                let random_index = if subs.len() > 0 {
                    random_range(0..subs.len())
                } else {
                    0
                };

                if let Some(sub) = subs.get(random_index).cloned() {
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
            .await
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
            .await
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
                                let sub = match self.generate(sub.name, interpreter).await {
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
        input: impl Into<String>,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let mut output: String = input.into();
        let mut prev_hashes = HashSet::new();

        let mut modified_interpreter = interpreter.lock().await;
        let funboy = Arc::new(self.clone());
        modified_interpreter.register(GET_SUB, GET_SUB_RULES, get_sub_command(funboy.clone()));
        modified_interpreter.register(ADD_SUBS, ADD_SUBS_RULES, add_subs_command(funboy.clone()));
        modified_interpreter.register(ASK_AI, ASK_AI_RULES, ask_ai_command(funboy));
        drop(modified_interpreter);

        for _ in 0..u8::MAX {
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
        prompt: impl Into<String>,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<OllamaResponse, FunboyError> {
        let prompt = self.generate(prompt, interpreter).await?;
        match self
            .ollama_generator
            .generate(&prompt, ollama_settings, model)
            .await
        {
            Ok(output) => Ok(OllamaResponse {
                prompt: prompt,
                generated_text: output.response,
            }),
            Err(e) => Err(FunboyError::Ollama(e.to_string())),
        }
    }

    pub async fn user_generate(
        &self,
        user_id: U,
        input: impl Into<String>,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let user_ctx = self.users.get_or_insert(user_id).await?;
        let Some(_guard) = FlagGuard::new(user_ctx.is_generating.clone()) else {
            return Err(FunboyError::UsageLimit(
                "You're already generating something, please wait until it's finished.".to_string(),
            ));
        };

        let cancel_token = {
            let mut cancel_token = user_ctx.cancel_generation.lock().await;
            *cancel_token = CancellationToken::new();
            cancel_token.clone()
        };

        let generate = self.generate(input, interpreter);

        let output = tokio::select! {
            result = generate => result,
            _ = cancel_token.cancelled() => {
                Err(FunboyError::GenerationCancelled)
            }
        };

        output
    }

    pub async fn user_generate_ollama(
        &self,
        user_id: U,
        prompt: impl Into<String>,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<OllamaResponse, FunboyError> {
        let user_ctx = self.users.get_or_insert(user_id).await?;
        let Some(_guard) = FlagGuard::new(user_ctx.is_generating.clone()) else {
            return Err(FunboyError::UsageLimit(
                "You're already generating something, please wait until it's finished.".to_string(),
            ));
        };

        let cancel_token = {
            let mut cancel_token = user_ctx.cancel_generation.lock().await;
            *cancel_token = CancellationToken::new();
            cancel_token.clone()
        };

        let ollama_settings = user_ctx.ollama_settings.lock().await.clone();
        let generate = self.generate_ollama(
            self.get_ollama_model().await,
            &ollama_settings,
            prompt,
            interpreter,
        );

        let output = tokio::select! {
            result = generate => result,
            _ = cancel_token.cancelled() => {
                Err(FunboyError::GenerationCancelled)
            }
        };

        output
    }

    pub async fn user_chat(
        &self,
        user_id: U,
        input: String,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let user_ctx = self.users.get_or_insert(user_id).await?;
        let Some(_guard) = FlagGuard::new(user_ctx.is_generating.clone()) else {
            return Err(FunboyError::UsageLimit(
                "You're already generating something, please wait until it's finished.".to_string(),
            ));
        };

        let cancel_token = {
            let mut cancel_token = user_ctx.cancel_generation.lock().await;
            *cancel_token = CancellationToken::new();
            cancel_token.clone()
        };

        let result = self.generate(&input, interpreter).await;
        let input = if let Ok(result) = result {
            result
        } else {
            input
        };

        let ollama_settings = user_ctx.ollama_settings.lock().await.clone();
        let model = self.get_ollama_model().await;
        let result = self
            .ollama_generator
            .chat(input, &ollama_settings, model, user_ctx);

        let output = tokio::select! {
            result = result => {
                 match result {
                    Ok(response) => Ok(response.message.content),
                    Err(e) => {
                        Err(FunboyError::Ollama(e.to_string()))
                    }
                }
            },
            _ = cancel_token.cancelled() => {
                Err(FunboyError::GenerationCancelled)
            }
        };

        output
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
                    Ok(gen_rand_num_inclusive::<T>(min, max).to_string())
                } else {
                    Ok(gen_rand_num_exclusive::<T>(min, max).to_string())
                }
            }
        }
        _ => Err("min and max values must be a number"),
    }
}
pub fn random_number(min: &str, max: &str, inclusive: bool) -> Result<String, FunboyError> {
    if min.contains('.') || max.contains('.') {
        match gen_rand_num_from_str::<f64>(min, max, inclusive) {
            Ok(result) => Ok(result),

            Err(e) => Err(FunboyError::UserInput(e.to_string())),
        }
    } else {
        match gen_rand_num_from_str::<i64>(min, max, inclusive) {
            Ok(result) => Ok(result),

            Err(e) => Err(FunboyError::UserInput(e.to_string())),
        }
    }
}

pub fn random_entry<'b>(list: &[&'b str]) -> Result<&'b str, FunboyError> {
    if list.len() < 2 {
        Err(FunboyError::UserInput(
            "list must contain at least two entries".to_string(),
        ))
    } else {
        let output = list[gen_rand_num_inclusive(0, list.len() - 1)];
        Ok(output)
    }
}

#[cfg(test)]
mod core {
    use super::*;
    use database::test::create_debug_db;
    use sqlx::PgPool;
    use std::panic;

    #[tokio::test]
    async fn random_number_produces_int_in_range() {
        for _ in 0..100 {
            let result = random_number("1", "6", true)
                .unwrap()
                .parse::<i64>()
                .unwrap();
            assert!((1..=6).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_produces_float() {
        for _ in 0..100 {
            let result = random_number("1.0", "6.0", true)
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((1.0..=6.0).contains(&result), "output outside of range");
        }
    }

    #[tokio::test]
    async fn random_number_fails_when_min_greater_than_max() {
        match random_number("6", "1", true) {
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
        match random_number("6", "6", true) {
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
        let result = random_entry(&["one", "two", "three", "four"]).unwrap();

        if !(&["one", "two", "three", "four"].contains(&result)) {
            panic!("array should contain result");
        }
    }

    #[tokio::test]
    async fn random_entry_fails_with_less_than_two_entries() {
        match random_entry(&["one"]) {
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
        PgPool::connect(database::DEBUG_DB_URL).await.unwrap()
    }

    impl FunboyUserId for u64 {}

    async fn get_funboy(pool: PgPool) -> Funboy<u64> {
        let db = create_debug_db(pool).await.unwrap();
        Funboy::new(db, Platform::Cli)
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
                &["A ^gtadj brown ^gtnoun ^gtverb^ed over the lazy dog."],
            )
            .await
            .unwrap();

        funboy.add_substitutes("gtadj", &["quick"]).await.unwrap();
        funboy.add_substitutes("gtnoun", &["fox"]).await.unwrap();
        funboy.add_substitutes("gtverb", &["jump"]).await.unwrap();

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
                &format!(
                    "{0}noun {0}noun {0}noun {0}noun {0}noun",
                    TemplateDelimiter::Plus.to_char()
                ),
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
            .generate(&format!(
                "{0}noun-1 {0}noun-1 {0}noun-2 {0}noun-2 {0}noun-2 {0}noun-999 {0}noun-999 {0}noun-999{0}noun-999{0}",
                TemplateDelimiter::Plus.to_char()),
                Arc::new(Mutex::new(FslInterpreter::new())),
            )
            .await
            .unwrap();

        dbg!(&output);

        let subs = output.split_whitespace().collect::<Vec<&str>>();
        assert!(subs[0] == subs[1]);
        assert!(subs[2] == subs[3]);
        assert!(subs[3] == subs[4]);
        assert!(subs[5] == subs[6]);
        assert!(subs[6] != subs[7]);
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
    async fn _generate_ollama_response() {
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

        println!("Ollama response: {}", generation_response.generated_text);
    }
}
