use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
};

use regex::Regex;
use strum_macros::EnumIter;

pub const VALID_TEMPLATE_CHARS: &str = "a-z0-9_";

#[derive(Debug, Copy, Clone, EnumIter)]
pub enum TemplateDelimiter {
    Caret,
    SingleQuote,
    BackTick,
}

impl TemplateDelimiter {
    pub fn to_char(&self) -> char {
        match self {
            TemplateDelimiter::Caret => '^',
            TemplateDelimiter::SingleQuote => '\'',
            TemplateDelimiter::BackTick => '`',
        }
    }

    pub fn to_regex_pattern(&self) -> String {
        match self {
            TemplateDelimiter::Caret => format!(r"\^[{}]+\^?", VALID_TEMPLATE_CHARS),
            TemplateDelimiter::SingleQuote => format!(r"\'[{}]+\'?", VALID_TEMPLATE_CHARS),
            TemplateDelimiter::BackTick => format!(r"\`[{}]+\`?", VALID_TEMPLATE_CHARS),
        }
    }
}

#[derive(Debug)]
pub struct TemplateSubstitutor {
    delimiter: TemplateDelimiter,
    regex: Regex,
    depth_limit: u16,
}

impl Default for TemplateSubstitutor {
    fn default() -> Self {
        let delimiter = TemplateDelimiter::Caret;
        Self {
            delimiter,
            regex: Regex::new(&delimiter.to_regex_pattern()).unwrap(),
            depth_limit: 255,
        }
    }
}

impl TemplateSubstitutor {
    pub fn new(delimiter: TemplateDelimiter) -> Self {
        Self {
            delimiter,
            regex: Regex::new(&delimiter.to_regex_pattern()).unwrap(),
            ..Default::default()
        }
    }
}

impl TemplateSubstitutor {
    pub async fn rename_template(&self, input: &str, old_name: &str, new_name: &str) -> String {
        let mut output = String::new();
        let mut i = 0;
        for template in self.regex.find_iter(&input[i..]) {
            output.push_str(&input[i..template.start()]);
            let matched = template.as_str();
            let template_name = matched[1..].trim_end_matches(self.delimiter.to_char());

            if old_name == template_name {
                output.push(self.delimiter.to_char());
                output.push_str(new_name);
                output.push_str(&matched[template_name.len() + 1..]);
            } else {
                output.push_str(matched);
            }

            i = template.end();
        }
        output.push_str(&input[i..]);
        output
    }

    /// Resolves templates with a single pass over input
    pub async fn substitute<F, Fut>(&self, input: &str, template_mapper: &F) -> String
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        println!("Incoming text: {}", input);
        let mut output = String::new();
        let mut start = 0;
        let mut end = 0;
        for template in self.regex.find_iter(&input[start..]) {
            let sub = template_mapper(
                template.as_str()[1..]
                    .trim_end_matches(self.delimiter.to_char())
                    .to_string(),
            )
            .await;

            match sub {
                Some(sub) => {
                    end = template.end();

                    println!("matched sub: {}", sub);
                    println!("current delimiter: {:?}", self.delimiter);
                    let segment = self.regex.replace(&input[start..end], &sub).into_owned();
                    println!("replacement: {}", segment);

                    start = template.end();

                    output.push_str(&segment);
                }
                None => {
                    println!("nothing matched");
                    output.push_str(template.as_str());
                    start = template.end();
                    end = template.end();
                }
            }
        }
        output.push_str(&input[end..]);
        println!("outgoing text: {}", output);
        output
    }

    /// Recursively resolves templates until none are present or depth limit or infinte cycle is reached
    pub async fn substitute_recursively<F, Fut>(&self, input: String, template_mapper: F) -> String
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let mut output = self.substitute(&input, &template_mapper).await;

        let mut previous_hashes = HashSet::new();

        for _ in 0..self.depth_limit {
            let mut hasher = DefaultHasher::new();
            output.hash(&mut hasher);
            let hash = hasher.finish();

            if !previous_hashes.insert(hash) {
                break;
            } else {
                output = self.substitute(&output, &template_mapper).await;
            }
        }

        output
    }
}

#[cfg(test)]
mod template_substitutor_test {
    use std::{collections::HashMap, sync::Arc};

    use super::*;

    #[tokio::test]
    async fn nested_templates() {
        let mut template_map = HashMap::new();
        template_map.insert("adj", "quick");
        template_map.insert("color_adj", "brown");
        template_map.insert("color", "^color_adj");
        template_map.insert("noun", "fox");
        template_map.insert("verb", "jump");
        template_map.insert(
            "sentence",
            "The ^adj ^color_adj ^noun ^verb^ed over the lazy dog.",
        );
        let template_map = Arc::new(template_map);
        let template_substitutor = TemplateSubstitutor::default();
        let output = template_substitutor
            .substitute_recursively("^sentence".to_string(), |template| {
                let template_map = template_map.clone();
                async move {
                    match template_map.get(template.as_str()) {
                        Some(sub) => Some(sub.to_string()),
                        None => None,
                    }
                }
            })
            .await;
        assert!(output == "The quick brown fox jumped over the lazy dog.");
        println!("OUTPUT: {}", output);
    }

    #[tokio::test]
    async fn close_templates() {
        let mut template_map = HashMap::new();
        template_map.insert("adj", "quick");
        template_map.insert("color_adj", "brown");
        template_map.insert("color", "^color_adj");
        template_map.insert("noun", "fox");
        template_map.insert("verb", "jump");
        template_map.insert(
            "sentence",
            "The^adj^^color_adj^^noun^^verb^edoverthelazydog.",
        );
        let template_map = Arc::new(template_map);
        let template_substitutor = TemplateSubstitutor::default();
        let output = template_substitutor
            .substitute_recursively("^sentence".to_string(), |template| {
                let template_map = template_map.clone();
                async move {
                    match template_map.get(template.as_str()) {
                        Some(sub) => Some(sub.to_string()),
                        None => None,
                    }
                }
            })
            .await;
        assert!(output == "Thequickbrownfoxjumpedoverthelazydog.");
        println!("OUTPUT: {}", output);
    }

    #[tokio::test]
    async fn non_existant_template() {
        let template_map: HashMap<String, String> = HashMap::new();
        let template_map = Arc::new(template_map);
        let template_substitutor = TemplateSubstitutor::default();
        let output = template_substitutor
            .substitute_recursively("^sentence".to_string(), |template| {
                let template_map = template_map.clone();
                async move {
                    match template_map.get(template.as_str()) {
                        Some(sub) => Some(sub.to_string()),
                        None => None,
                    }
                }
            })
            .await;
        assert!(output == "^sentence");
        println!("OUTPUT: {}", output);
    }

    #[tokio::test]
    async fn recursive_templates() {
        let mut template_map = HashMap::new();
        template_map.insert("over_here", "^back_there");
        template_map.insert("over_there", "^over_here");
        template_map.insert("back_there", "^over_there");
        let template_map = Arc::new(template_map);
        let template_substitutor = TemplateSubstitutor::default();
        let output = template_substitutor
            .substitute_recursively("^over_here".to_string(), |template| {
                let template_map = template_map.clone();
                async move {
                    match template_map.get(template.as_str()) {
                        Some(sub) => Some(sub.to_string()),
                        None => None,
                    }
                }
            })
            .await;
        println!("OUTPUT: {}", output);
    }
}
