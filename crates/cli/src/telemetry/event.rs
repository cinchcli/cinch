//! Vendor-agnostic event representation.

use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub properties: Map<String, Value>,
}

impl Event {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: Map::new(),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.properties.insert(key.to_string(), value.into());
        self
    }
}
