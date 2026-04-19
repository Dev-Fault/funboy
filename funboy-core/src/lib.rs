use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_recursion::async_recursion;
use clap::ValueEnum;
use fsl_interpreter::FslInterpreter;
use moka::future::{Cache, CacheBuilder};
use ollama_rs::models::ModelInfo;
use rand::{Rng, distr::uniform::SampleUniform, random_range};
use regex::Regex;
use strum_macros::{Display, EnumString};
use tokio::sync::Mutex;

use crate::{
    database::{
        FunboyDatabase, KeySize, Limit, OrderBy, Platform, Substitute, SubstituteReceipt, Template,
        TemplateReceipt,
    },
    interpreter::{
        ASK_AI, ASK_AI_RULES, GET_SUB, GET_SUB_RULES, create_ask_ai_command, create_get_sub_command,
    },
    ollama::{OllamaGenerator, OllamaSettings},
    template_substitutor::{TemplateDelimiter, TemplateSubstitutor, VALID_TEMPLATE_CHARS},
};

pub mod commands;
pub mod database;
pub mod format;
pub mod interpreter;
pub mod ollama;
pub mod rate_limiter;
pub mod template_substitutor;

#[derive(Debug, Clone)]
pub enum PermissionError {
    CannotGrantOwnerPermission,
    CannotRevokeOwnerPermission,
    CannotRevokePermissionsFromOwner,
}

impl ToString for PermissionError {
    fn to_string(&self) -> String {
        match self {
            PermissionError::CannotGrantOwnerPermission => "owner permission cannot be granted",
            PermissionError::CannotRevokeOwnerPermission => "owner permission cannot be revoked",
            PermissionError::CannotRevokePermissionsFromOwner => {
                "permissions cannot be revoked from owner"
            }
        }
        .to_owned()
    }
}

#[derive(Debug, Clone)]
pub enum FunboyError {
    Interpreter(String),
    Ollama(String),
    Database(String),
    UserInput(String),
    UsageLimit(String),
    Permission(PermissionError),
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
            FunboyError::UsageLimit(e) => e.clone(),
            FunboyError::Permission(permission_error) => permission_error.to_string(),
        }
    }
}

impl From<sqlx::Error> for FunboyError {
    fn from(value: sqlx::Error) -> Self {
        eprintln!("{}", value);
        FunboyError::Database(value.to_string())
    }
}

pub struct OllamaResponse {
    pub prompt: String,
    pub generated_text: String,
}

#[derive(Debug, Clone)]
pub enum Request {
    GenerateFile,
    UploadSub(String),
}

#[derive(Debug, Clone)]
pub struct UserCtx {
    pub is_generating: Arc<AtomicBool>,
    pub ollama_settings: Arc<Mutex<OllamaSettings>>,
    pub pending_requests: Arc<Mutex<Vec<Request>>>,
    permissions: Arc<Mutex<Permissions>>,
}

impl Default for UserCtx {
    fn default() -> Self {
        Self {
            is_generating: Default::default(),
            ollama_settings: Default::default(),
            pending_requests: Default::default(),
            permissions: Default::default(),
        }
    }
}

impl UserCtx {
    pub fn new() -> UserCtx {
        Self {
            is_generating: Arc::new(AtomicBool::new(false)),
            ..Default::default()
        }
    }

    pub fn with_permissions(mut self, permissions: Permissions) -> UserCtx {
        self.permissions = Arc::new(Mutex::new(permissions));
        self
    }

    pub fn with_ollama_settings(mut self, settings: OllamaSettings) -> UserCtx {
        self.ollama_settings = Arc::new(Mutex::new(settings));
        self
    }
}

struct FlagGuard {
    flag: Arc<AtomicBool>,
}

impl FlagGuard {
    fn new(flag: Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .ok()
            .map(|_| Self { flag })
    }
}

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Relaxed);
    }
}

pub trait UserId: Eq + Hash + Send + Sync + Clone + ToString + 'static {}

#[derive(Debug, Clone)]
pub struct UserMap<U: UserId> {
    platform: Platform,
    db: FunboyDatabase,
    users: Arc<Mutex<HashMap<U, UserCtx>>>,
}

impl<U: UserId> UserMap<U> {
    pub fn new(platform: Platform, db: FunboyDatabase) -> Self {
        Self {
            platform,
            db,
            users: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_or_insert(&self, user_id: U) -> UserCtx {
        let users = self.users.lock().await;
        if let Some(user_ctx) = users.get(&user_id) {
            user_ctx.clone()
        } else {
            drop(users);

            let user_ctx = match self
                .db
                .create_user(self.platform, user_id.to_string())
                .await
            {
                Ok(user) => match self.db.get_permissions(user.id).await {
                    Ok(permissions) => UserCtx::new()
                        .with_permissions(permissions)
                        .with_ollama_settings(OllamaSettings::default()),
                    Err(e) => {
                        eprintln!("{e}");
                        UserCtx::default()
                    }
                },
                Err(e) => {
                    eprintln!("{e}");
                    UserCtx::default()
                }
            };

            let mut users = self.users.lock().await;
            users.entry(user_id).or_insert(user_ctx).clone()
        }
    }

    pub async fn grant_all_permissions(&mut self, user_id: U) {
        let user_ctx = self.get_or_insert(user_id.clone()).await;

        let mut permissions = user_ctx.permissions.lock().await;
        permissions.0 = Permissions::all().0;

        let result = self
            .db
            .overwrite_user_permissions(self.platform, user_id.to_string(), &Permissions::all())
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }
    }

    pub async fn grant_permissions(
        &mut self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), PermissionError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotGrantOwnerPermission);
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await;

        let mut current_permissions = user_ctx.permissions.lock().await;
        for permission in permissions {
            current_permissions.0.insert(*permission);
        }

        let result = self
            .db
            .overwrite_user_permissions(self.platform, user_id.to_string(), &current_permissions)
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }

        Ok(())
    }

    pub async fn revoke_permissions(
        &mut self,
        user_id: U,
        permissions: &[Permission],
    ) -> Result<(), PermissionError> {
        if permissions.contains(&Permission::Owner) {
            return Err(PermissionError::CannotRevokeOwnerPermission);
        }

        let user_ctx = self.get_or_insert(user_id.clone()).await;
        let mut current_permissions = user_ctx.permissions.lock().await;

        if current_permissions.is_host() {
            return Err(PermissionError::CannotRevokePermissionsFromOwner);
        }

        for permission in permissions {
            current_permissions.0.remove(permission);
        }

        let result = self
            .db
            .overwrite_user_permissions(self.platform, user_id.to_string(), &current_permissions)
            .await;

        if let Err(e) = result {
            eprintln!("{}", e.to_string())
        }

        Ok(())
    }

    pub async fn get_permissions(&self, user_id: U) -> Permissions {
        let user_ctx = self.get_or_insert(user_id.clone()).await;
        let permissions = user_ctx.permissions.lock().await;
        permissions.clone()
    }
}

pub const MAX_TEMPLATE_LENGTH: usize = 255;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, Display, EnumString)]
pub enum Permission {
    Owner,
    File,
    Create,
    Update,
    Generate,
    Ollama,
    Grant,
    Revoke,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Owner => "owner",
            Permission::File => "file",
            Permission::Create => "create",
            Permission::Update => "update",
            Permission::Generate => "generate",
            Permission::Ollama => "ollama",
            Permission::Grant => "grant",
            Permission::Revoke => "revoke",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Permissions(HashSet<Permission>);

impl ToString for Permissions {
    fn to_string(&self) -> String {
        let mut permissions: Vec<String> = vec![];

        for permission in &self.0 {
            permissions.push(permission.as_str().to_owned());
        }

        permissions.join(", ")
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Permissions::user()
    }
}

impl Permissions {
    pub fn from(permissions: HashSet<Permission>) -> Self {
        Self(permissions)
    }

    pub fn all() -> Self {
        Permissions(HashSet::from([
            Permission::Owner,
            Permission::File,
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
            Permission::Grant,
            Permission::Revoke,
        ]))
    }

    pub fn admin() -> Self {
        Permissions(HashSet::from([
            Permission::File,
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
            Permission::Grant,
            Permission::Revoke,
        ]))
    }

    pub fn trusted_user() -> Self {
        Permissions(HashSet::from([
            Permission::Create,
            Permission::Update,
            Permission::Generate,
            Permission::Ollama,
        ]))
    }

    pub fn user() -> Self {
        Permissions(HashSet::from([Permission::Generate, Permission::Ollama]))
    }

    pub fn none() -> Self {
        Permissions(HashSet::new())
    }

    pub fn can_use_files(&self) -> bool {
        self.0.contains(&Permission::File)
    }

    pub fn can_generate(&self) -> bool {
        self.0.contains(&Permission::Generate)
    }

    pub fn can_create(&self) -> bool {
        self.0.contains(&Permission::Create)
    }

    pub fn can_update(&self) -> bool {
        self.0.contains(&Permission::Update)
    }

    pub fn can_use_ollama(&self) -> bool {
        self.0.contains(&Permission::Ollama)
    }

    pub fn can_grant(&self) -> bool {
        self.0.contains(&Permission::Grant)
    }

    pub fn can_revoke(&self) -> bool {
        self.0.contains(&Permission::Revoke)
    }

    pub fn has_permission(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }

    pub fn is_host(&self) -> bool {
        self.0.contains(&Permission::Owner)
    }

    pub fn get_lacking(&self, required_permissions: &[Permission]) -> Permissions {
        Permissions::from(
            required_permissions
                .iter()
                .filter(|p| !self.0.contains(p))
                .map(|p| p.to_owned())
                .collect(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Funboy<U: UserId> {
    funboy_db: FunboyDatabase,
    ollama_model: Arc<Mutex<Option<String>>>,
    ollama_generator: OllamaGenerator,
    valid_template_regex: Regex,
    pub users: UserMap<U>,
    random_sub_cache: Arc<Cache<String, Vec<Substitute>>>,
}

impl<U: UserId> Funboy<U> {
    pub fn new(funboy_db: FunboyDatabase, platform: Platform) -> Self {
        Self {
            funboy_db: funboy_db.clone(),
            ollama_generator: OllamaGenerator::default(),
            ollama_model: Arc::new(Mutex::new(None)),
            valid_template_regex: Regex::new(&format!("^[{}]+$", VALID_TEMPLATE_CHARS)).unwrap(),
            users: UserMap::new(platform, funboy_db),
            random_sub_cache: Arc::new(
                CacheBuilder::new(20)
                    .time_to_live(Duration::from_secs(60))
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
        modified_interpreter.add_command(
            GET_SUB,
            GET_SUB_RULES,
            create_get_sub_command(funboy.clone()),
        );
        modified_interpreter.add_command(ASK_AI, ASK_AI_RULES, create_ask_ai_command(funboy));
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
        input: &str,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<String, FunboyError> {
        let user_ctx = self.users.get_or_insert(user_id).await;
        let Some(_guard) = FlagGuard::new(user_ctx.is_generating.clone()) else {
            return Err(FunboyError::UsageLimit(
                "You're already generating something, please wait until it's finished.".to_string(),
            ));
        };

        let output = self.generate(input, interpreter).await;
        output
    }

    pub async fn user_generate_ollama(
        &self,
        user_id: U,
        prompt: &str,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<OllamaResponse, FunboyError> {
        let user_ctx = self.users.get_or_insert(user_id).await;
        let Some(_guard) = FlagGuard::new(user_ctx.is_generating.clone()) else {
            return Err(FunboyError::UsageLimit(
                "You're already generating something, please wait until it's finished.".to_string(),
            ));
        };
        let ollama_settings = user_ctx.ollama_settings.lock().await.clone();
        let output = self
            .generate_ollama(
                self.get_ollama_model().await,
                &ollama_settings,
                prompt,
                interpreter,
            )
            .await;
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

    impl UserId for u64 {}

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

        println!("Ollama response: {}", generation_response.generated_text);
    }
}
