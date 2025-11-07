use std::{fs::File, path::Path};

use serde::{Deserialize, Serialize};

const FSL_DOCUMENTATION: &str = include_str!("../../fsl_documentation.json");

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
    let command_documentation: CommandDocumentation =
        serde_json::from_str(FSL_DOCUMENTATION).expect("fsl documentation should be valid json");
    command_documentation.commands
}
