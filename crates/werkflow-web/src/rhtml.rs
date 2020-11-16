use std::{collections::HashMap, path::Path, str::FromStr,  fs};

use log::{warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncReadExt, fs::File};
use werkflow_scripting::Script;

use anyhow::{Result, anyhow};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Library {
    preamble: String,
    lib: HashMap<String, ScriptTemplate>
}

impl Library {
    pub fn get(&self, name : &str) -> Result<ScriptTemplate> {
        match self.lib.get(name) {
            Some(script) => {
                Ok(script.clone())
            }
            None => {
                Err(anyhow!("Could not load script"))
            }
        }
    }
    pub async fn load_directory(path: impl AsRef<Path>) -> Result<Library> {
        let mut lib = Library::default();

        let paths = fs::read_dir(path)?;

        for p in paths {
            let file = p.unwrap();

            let file_name = file.file_name();
            let file_name = file_name.to_str().unwrap();
            
            if file_name.ends_with(".rhtml"){
                let template_name = file_name.split(".").collect::<Vec<&str>>()[0];
                match ScriptTemplate::from_file(file.path()).await {
                    Ok(mut script) => {
                        script.source = file_name.to_string();
                        lib.lib.insert(template_name.into(), script);
                    }
                    Err(err) => {
                        warn!("Couldn't load script into library. name: {} error: {}", template_name, err);
                    }
                }
            }
        }
        

        Ok(lib)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptTemplate {
    source: String,
    pub script: Script,
    pub template: String
}
/// rhtml templates take the form
/// ---!
/// #{
///   "result" : "This the result of the script",
///   "body" : state["request.body"]
/// }
/// !---
/// <div>Here's the result: {{result}}</div>
/// <div>And you sent {{body}}</div>
impl ScriptTemplate {
    pub async fn from_file(path: impl AsRef<Path>) -> Result<ScriptTemplate> {
        let mut file = File::open(&path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        Ok(contents.parse::<ScriptTemplate>()?)
    }    
}

impl FromStr for ScriptTemplate {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"(?ms)\s*!---\n(.*)\s*---!\n(.*)").unwrap();

        let cap = re.captures(s);

        if let Some(cap) = cap {
            Ok(ScriptTemplate {
                source: "String".into(),
                script: Script::new(&cap[1]),
                template: cap[2].to_string()
            })
        } else {
            Err(anyhow!("Error parsing rhai file, please see ... for proper format."))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_regex() {
        let template = r#"
!---
// Comment
#{ "test":"This is a test" }
---!
<div>{{ test }}</div>
<div>Line 2</div>
"#;

        let template = template.parse::<ScriptTemplate>().expect("can parse template");

        assert_eq!(template.script.body, "// Comment\n#{ \"test\":\"This is a test\" }\n");
        assert_eq!(template.template, "<div>{{ test }}</div>\n<div>Line 2</div>\n");
    }
}