use std::sync::mpsc::Receiver;
use std::sync::mpsc::channel;
use notify::{DebouncedEvent, ReadDirectoryChangesWatcher, RecursiveMode, Watcher, watcher};
use std::{time::Duration, collections::HashMap, fs, path::Path, str::FromStr};

use AsRef;
use log::{info, warn};
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
    pub fn rename(&mut self, name: &str, new_name: &str) {
        let script = self.lib.remove(name).unwrap();
        self.lib.insert(new_name.into(), script);
    }
    pub fn remove(&mut self, name: &str) {
        self.lib.remove(name);
    }
    pub async fn update_from_file(&mut self, path: impl AsRef<Path>) {
        let path = &path.as_ref().to_path_buf();
        let file_name = path.file_name().unwrap().to_str().unwrap();

        if file_name.ends_with(".rhtml"){
            let template_name = file_name.split(".").collect::<Vec<&str>>()[0];
            match ScriptTemplate::from_file(path).await {
                Ok(mut script) => {
                    script.source = file_name.to_string();
                    self.lib.insert(template_name.into(), script);
                }
                Err(err) => {
                    warn!("Couldn't load script into library. name: {} error: {}", template_name, err);
                }
            }
        }
    }
    pub async fn watch_directory(path: impl AsRef<Path>) -> Result<(ReadDirectoryChangesWatcher, Receiver<DebouncedEvent>, Library)> {
        
        let lib = Library::load_directory(&path).await?;
        let (tx, rx) = channel::<DebouncedEvent>();

        // Create a watcher object, delivering debounced events.
        // The notification back-end is selected based on the platform.
        let mut watcher = watcher(tx, Duration::from_secs(3)).unwrap();

        // Add a path to be watched. All files and directories at that path and
        // below will be monitored for changes.
        watcher.watch(path, RecursiveMode::Recursive).unwrap();

        /*loop {
            match rx.recv() {
                Ok(event) => println!("{:?}", event),
                Err(e) => println!("watch error: {:?}", e),
            }
        }*/

        Ok((watcher, rx, lib))
    }
    pub async fn load_directory(path: impl AsRef<Path>) -> Result<Library> {
        

        let mut lib = Library::default();

        info!("Loading directory {} into script Library", path.as_ref().to_str().unwrap());
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
/// ---!
/// <div>Here's the result: {{result}}</div>
/// <div>And you sent {{body}}</div>
impl ScriptTemplate {
    pub async fn from_file(path: impl AsRef<Path>) -> Result<ScriptTemplate> {
        info!("loading script template from file {}", path.as_ref().to_str().unwrap());
        let mut file = File::open(&path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        Ok(contents.parse::<ScriptTemplate>()?)
    }    
}

impl FromStr for ScriptTemplate {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        println!("{}",s);
        let re = Regex::new(r"(?ms)\s*!---\r?\n(.*)\s*---!\r?\n(.*)").unwrap();

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