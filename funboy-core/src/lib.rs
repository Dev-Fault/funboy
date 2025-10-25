pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

pub enum FunboyError {
    Interpolator(String),
    Interpreter(String),
    ai(String),
    Database(String),
}

pub struct Funboy {}

impl Funboy {
    pub async fn generate_random_number(min: f64, max: f64) -> f64 {
        todo!()
    }

    // Previously "random_word"
    pub async fn select_random_entry<'a>(list: &[&'a str]) -> &'a str {
        todo!()
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
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
