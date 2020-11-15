
use crate::{ScriptHost, ScriptHostPlugin};
use crate::HashMap;

/// State for the script host
#[derive(Clone, Debug)]
pub struct HostState {
    fields: HashMap<String, String>,
}


impl ScriptHostPlugin for HostState {
    fn init(&self, host : &mut ScriptHost){
        host.engine.register_type::<HostState>()
            .register_indexer_get(HostState::get_field)
            .register_indexer_set(HostState::set_field)
            .register_indexer_get(HostState::get_field_int)
            .register_indexer_set(HostState::set_field_int)
            .register_indexer_get(HostState::get_field_bool)
            .register_indexer_set(HostState::set_field_bool)
            .register_indexer_get(HostState::get_field_float)
            .register_indexer_set(HostState::set_field_float);
    }
}

impl HostState {
    
    pub fn new() -> Self {
        HostState {
            fields: HashMap::new(),
        }
    }
    fn get_field(&mut self, index: String) -> String {
        if let Some(field_value) = self.fields.get(&index) {
            field_value.to_string()
        } else {
            "".to_string()
        }
    }
    fn set_field(&mut self, index: String, value: String) {
        self.fields.insert(index, value);
    }
    fn get_field_int(&mut self, index: String) -> i64 {
        if let Some(num) = self.fields.get(&index) {
            num.parse().unwrap()
        } else {
            0_i64
        }
    }
    fn set_field_int(&mut self, index: String, value: i64) {
        self.fields.insert(index, value.to_string());
    }
    fn get_field_bool(&mut self, index: String) -> bool {
        if let Some(b) = self.fields.get(&index) {
            b.parse().unwrap()
        } else {
            false
        }
    }
    fn set_field_bool(&mut self, index: String, value: bool) {
        self.fields.insert(index, value.to_string());
    }
    fn get_field_float(&mut self, index: String) -> f64 {
        if let Some(b) = self.fields.get(&index) {
            b.parse().unwrap()
        } else {
            0_f64
        }
    }
    fn set_field_float(&mut self, index: String, value: f64) {
        self.fields.insert(index, value.to_string());
    }
}