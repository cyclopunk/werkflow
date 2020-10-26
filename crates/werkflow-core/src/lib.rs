use std::collections::HashMap;

#[derive(Debug,Clone)]
pub enum HttpAction {
    Get(String),
    Post(String),
    GetWithHeaders(String, HashMap<String, String>),
    PostWithHeaders(String, HashMap<String, String>)
}
