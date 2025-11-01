use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
};

use regex::Regex;

#[derive(Debug)]
pub struct TemplateSubstitutor {
    regex: Regex,
    depth_limit: u16,
}

impl Default for TemplateSubstitutor {
    fn default() -> Self {
        let pattern = r"\^[\w-]+\^?";
        Self {
            regex: Regex::new(&pattern).unwrap(),
            depth_limit: 255,
        }
    }
}

impl TemplateSubstitutor {
    async fn substitute_templates<F, Fut>(&self, input: &str, template_map: &F) -> String
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let mut output = String::new();
        let mut start = 0;
        let mut end = 0;
        for template in self.regex.find_iter(&input[start..]) {
            let sub = match template_map(template.as_str()[1..].trim_end_matches('^').to_string())
                .await
            {
                Some(sub) => sub,
                None => template.as_str().to_string(),
            };

            end = template.end();

            let segment = self.regex.replace(&input[start..end], &sub).into_owned();

            start = template.end();

            output.push_str(&segment);
        }
        output.push_str(&input[end..input.len()]);
        output
    }

    pub async fn run<F, Fut>(&self, input: String, template_map: F) -> String
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let mut output = self.substitute_templates(&input, &template_map).await;

        let mut previous_hashes = HashSet::new();

        for _ in 0..self.depth_limit {
            let mut hasher = DefaultHasher::new();
            output.hash(&mut hasher);
            let hash = hasher.finish();

            if !previous_hashes.insert(hash) {
                eprintln!("Warning: cycle detected in template expansion");
                break;
            }

            let next_output = self.substitute_templates(&output, &template_map).await;
            if next_output == output {
                break;
            } else {
                output = next_output;
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
            .run("^sentence".to_string(), |template| {
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
            .run("^sentence".to_string(), |template| {
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
            .run("^sentence".to_string(), |template| {
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
            .run("^over_here".to_string(), |template| {
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
