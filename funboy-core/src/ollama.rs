use ollama_rs::{
    Ollama,
    error::OllamaError,
    generation::completion::{GenerationResponse, request::GenerationRequest},
    models::{LocalModel, ModelInfo, ModelOptions},
};

const DEFAULT_SYSTEM_PROMPT: &str = "";
const DEFAULT_TEMPLATE: &str = "{{ .Prompt }}";
const DEFAULT_MAX_PREDICT: u16 = 200;
const PARAMETER_NOT_SET_TEXT: &str = "Unset";
pub const MAX_PREDICT: u16 = 2000;

#[derive(Copy, Clone)]
pub struct OllamaParameters {
    pub temperature: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
}

impl OllamaParameters {
    pub fn new(
        temperature: Option<f32>,
        repeat_penalty: Option<f32>,
        top_k: Option<u32>,
        top_p: Option<f32>,
    ) -> Self {
        Self {
            temperature,
            repeat_penalty,
            top_k,
            top_p,
        }
    }

    pub fn reset(&mut self) {
        self.temperature = None;
        self.repeat_penalty = None;
        self.top_k = None;
        self.top_p = None;
    }

    pub fn param_to_string<P: ToString>(param: Option<P>) -> String {
        if param.is_some() {
            return param.unwrap().to_string();
        } else {
            return PARAMETER_NOT_SET_TEXT.to_string();
        }
    }
}

impl Default for OllamaParameters {
    fn default() -> Self {
        Self {
            temperature: None,
            repeat_penalty: None,
            top_k: None,
            top_p: None,
        }
    }
}

#[derive(Clone)]
pub struct OllamaSettings {
    system_prompt: String,
    template: String,
    output_limit: u16,
    parameters: OllamaParameters,
}

impl OllamaSettings {
    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.system_prompt = prompt.to_string();
    }

    pub fn reset_system_prompt(&mut self) {
        self.system_prompt = DEFAULT_SYSTEM_PROMPT.to_string();
    }

    pub fn set_template(&mut self, template: &str) {
        self.template = template.to_string();
    }

    pub fn reset_template(&mut self) {
        self.template = DEFAULT_TEMPLATE.to_string();
    }

    pub fn set_output_limit(&mut self, limit: u16) -> bool {
        if limit > MAX_PREDICT {
            false
        } else {
            self.output_limit = limit;
            true
        }
    }

    pub fn set_parameters(&mut self, parameters: OllamaParameters) {
        self.parameters = parameters;
    }

    pub fn reset_parameters(&mut self) {
        self.parameters.reset();
    }

    pub fn set_temperature(&mut self, temperature: f32) {
        self.parameters.temperature = Some(temperature);
    }

    pub fn set_repeat_penalty(&mut self, repeat_penalty: f32) {
        self.parameters.repeat_penalty = Some(repeat_penalty);
    }

    pub fn set_top_k(&mut self, top_k: u32) {
        self.parameters.top_k = Some(top_k);
    }

    pub fn set_top_p(&mut self, top_p: f32) {
        self.parameters.top_p = Some(top_p);
    }
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            template: DEFAULT_TEMPLATE.to_string(),
            output_limit: DEFAULT_MAX_PREDICT,
            parameters: OllamaParameters::default(),
        }
    }
}

impl ToString for OllamaSettings {
    fn to_string(&self) -> String {
        format!(
            "System Prompt: {}\nTemplate: {}\nOutput Limit: {}\nTemperature: {}\nRepeat Penalty: {}\nTop_k: {}\nTop_p: {}",
            self.system_prompt,
            self.template,
            self.output_limit,
            OllamaParameters::param_to_string(self.parameters.temperature),
            OllamaParameters::param_to_string(self.parameters.repeat_penalty),
            OllamaParameters::param_to_string(self.parameters.top_k),
            OllamaParameters::param_to_string(self.parameters.top_p),
        )
    }
}

pub struct OllamaGenerator {
    ollama: Ollama,
}

impl OllamaGenerator {
    pub async fn get_models(&self) -> Result<Vec<LocalModel>, OllamaError> {
        self.ollama.list_local_models().await
    }

    pub async fn get_model_info(&self, model: String) -> Result<ModelInfo, OllamaError> {
        self.ollama.show_model_info(model).await
    }

    pub async fn get_default_model(&self) -> Option<String> {
        let available_models = self.get_models().await;
        match available_models {
            Ok(models) => Some(models[0].name.clone()),
            Err(_) => None,
        }
    }

    fn generate_options(&self, ollama_settings: &OllamaSettings) -> ModelOptions {
        let mut options = ModelOptions::default();
        let parameters = &ollama_settings.parameters;
        options = options.num_predict(ollama_settings.output_limit.into());
        if let Some(temperature) = parameters.temperature {
            options = options.temperature(temperature);
        }
        if let Some(repeat_penalty) = parameters.repeat_penalty {
            options = options.repeat_penalty(repeat_penalty);
        }
        if let Some(top_k) = parameters.top_k {
            options = options.top_k(top_k);
        }
        if let Some(top_p) = parameters.top_p {
            options = options.top_p(top_p);
        }
        options
    }

    pub async fn generate(
        &self,
        prompt: &str,
        ollama_settings: &OllamaSettings,
        model: Option<String>,
    ) -> Result<GenerationResponse, OllamaError> {
        let override_options = self.generate_options(&ollama_settings);
        let model = match model {
            Some(name) => name.to_string(),
            None => {
                let available_models = self.get_models().await;
                match available_models {
                    Ok(models) => models[0].name.clone(),
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        };

        let mut request = GenerationRequest::new(model, prompt).options(override_options);
        request = request.system(ollama_settings.system_prompt.clone());
        request = request.template(ollama_settings.template.clone());
        self.ollama.generate(request).await
    }
}

impl Default for OllamaGenerator {
    fn default() -> Self {
        Self {
            ollama: Ollama::default(),
        }
    }
}
