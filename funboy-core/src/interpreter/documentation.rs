use std::{fs::File, path::Path};

use serde::{Deserialize, Serialize};

const FSL_DOCUMENTATION_PATH: &str = "fsl_documentation.json";

#[derive(Debug, Deserialize, Serialize)]
struct CommandDocumentation {
    commands: Vec<CommandInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub argument_count: String,
    pub argument_types: String,
    pub description: String,
    pub examples: Vec<String>,
}

pub fn get_command_documentation() -> Vec<CommandInfo> {
    let path = Path::new(FSL_DOCUMENTATION_PATH);
    let documenation = File::open(path).expect("fsl documentation file should exist");
    let command_documentation: CommandDocumentation =
        serde_json::from_reader(documenation).expect("fsl documentation should be valid json");
    command_documentation.commands
}
