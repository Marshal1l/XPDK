//! Configuration utilities for XPDK

use crate::{Error, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Configuration format
#[derive(Debug, Clone, Copy)]
pub enum ConfigFormat {
    Json,
    Toml,
    Yaml,
}

/// Configuration manager
pub struct ConfigManager {
    /// Configuration values
    values: HashMap<String, ConfigValue>,
    /// File path
    file_path: Option<String>,
    /// Format
    format: ConfigFormat,
}

/// Configuration value
#[derive(Debug, Clone)]
pub enum ConfigValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<ConfigValue>),
    Object(HashMap<String, ConfigValue>),
}

impl ConfigValue {
    /// Get boolean value
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            ConfigValue::Bool(value) => Ok(*value),
            _ => Err(Error::InvalidConfig("Not a boolean value".to_string())),
        }
    }

    /// Get integer value
    pub fn as_integer(&self) -> Result<i64> {
        match self {
            ConfigValue::Integer(value) => Ok(*value),
            ConfigValue::Float(value) => Ok(*value as i64),
            _ => Err(Error::InvalidConfig("Not an integer value".to_string())),
        }
    }

    /// Get float value
    pub fn as_float(&self) -> Result<f64> {
        match self {
            ConfigValue::Float(value) => Ok(*value),
            ConfigValue::Integer(value) => Ok(*value as f64),
            _ => Err(Error::InvalidConfig("Not a float value".to_string())),
        }
    }

    /// Get string value
    pub fn as_string(&self) -> Result<String> {
        match self {
            ConfigValue::String(value) => Ok(value.clone()),
            _ => Err(Error::InvalidConfig("Not a string value".to_string())),
        }
    }

    /// Get array value
    pub fn as_array(&self) -> Result<Vec<ConfigValue>> {
        match self {
            ConfigValue::Array(value) => Ok(value.clone()),
            _ => Err(Error::InvalidConfig("Not an array value".to_string())),
        }
    }

    /// Get object value
    pub fn as_object(&self) -> Result<HashMap<String, ConfigValue>> {
        match self {
            ConfigValue::Object(value) => Ok(value.clone()),
            _ => Err(Error::InvalidConfig("Not an object value".to_string())),
        }
    }
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            file_path: None,
            format: ConfigFormat::Json,
        }
    }

    /// Create configuration manager from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let format = Self::detect_format(path)?;

        let content = fs::read_to_string(path).map_err(|e| Error::IoError(e))?;

        let values = Self::parse_config(&content, format)?;

        Ok(Self {
            values,
            file_path: Some(path.to_string_lossy().to_string()),
            format,
        })
    }

    /// Set a configuration value
    pub fn set(&mut self, key: &str, value: ConfigValue) {
        self.values.insert(key.to_string(), value);
    }

    /// Get a configuration value
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }

    /// Get a configuration value with default
    pub fn get_or(&self, key: &str, default: ConfigValue) -> ConfigValue {
        self.get(key).cloned().unwrap_or(default)
    }

    /// Remove a configuration value
    pub fn remove(&mut self, key: &str) -> Option<ConfigValue> {
        self.values.remove(key)
    }

    /// Check if key exists
    pub fn contains_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Get all keys
    pub fn keys(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }

    /// Save configuration to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let format = Self::detect_format(path).unwrap_or(self.format);
        let content = Self::serialize_config(&self.values, format)?;

        fs::write(path, content).map_err(|e| Error::IoError(e))?;

        Ok(())
    }

    /// Reload configuration from file
    pub fn reload(&mut self) -> Result<()> {
        if let Some(ref path) = self.file_path {
            let content = fs::read_to_string(path).map_err(|e| Error::IoError(e))?;

            let values = Self::parse_config(&content, self.format)?;
            self.values = values;

            Ok(())
        } else {
            Err(Error::InvalidConfig("No file path set".to_string()))
        }
    }

    /// Detect configuration format from file extension
    fn detect_format(path: &Path) -> Result<ConfigFormat> {
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            match extension.to_lowercase().as_str() {
                "json" => Ok(ConfigFormat::Json),
                "toml" => Ok(ConfigFormat::Toml),
                "yaml" | "yml" => Ok(ConfigFormat::Yaml),
                _ => Err(Error::InvalidConfig(
                    "Unknown configuration format".to_string(),
                )),
            }
        } else {
            Err(Error::InvalidConfig("No file extension found".to_string()))
        }
    }

    /// Parse configuration content
    fn parse_config(content: &str, format: ConfigFormat) -> Result<HashMap<String, ConfigValue>> {
        match format {
            ConfigFormat::Json => Self::parse_json(content),
            ConfigFormat::Toml => Self::parse_toml(content),
            ConfigFormat::Yaml => Self::parse_yaml(content),
        }
    }

    /// Serialize configuration to content
    fn serialize_config(
        values: &HashMap<String, ConfigValue>,
        format: ConfigFormat,
    ) -> Result<String> {
        match format {
            ConfigFormat::Json => Self::serialize_json(values),
            ConfigFormat::Toml => Self::serialize_toml(values),
            ConfigFormat::Yaml => Self::serialize_yaml(values),
        }
    }

    /// Parse JSON configuration
    fn parse_json(_content: &str) -> Result<HashMap<String, ConfigValue>> {
        // Simplified JSON parsing
        // In a real implementation, you would use serde_json
        let values = HashMap::new();

        // For now, return empty config
        // This is a placeholder implementation
        Ok(values)
    }

    /// Parse TOML configuration
    fn parse_toml(_content: &str) -> Result<HashMap<String, ConfigValue>> {
        // Simplified TOML parsing
        // In a real implementation, you would use toml
        let values = HashMap::new();

        // For now, return empty config
        // This is a placeholder implementation
        Ok(values)
    }

    /// Parse YAML configuration
    fn parse_yaml(_content: &str) -> Result<HashMap<String, ConfigValue>> {
        // Simplified YAML parsing
        // In a real implementation, you would use serde_yaml
        let values = HashMap::new();

        // For now, return empty config
        // This is a placeholder implementation
        Ok(values)
    }

    /// Serialize to JSON
    fn serialize_json(_values: &HashMap<String, ConfigValue>) -> Result<String> {
        // Simplified JSON serialization
        // In a real implementation, you would use serde_json
        Ok("{\n}\n".to_string())
    }

    /// Serialize to TOML
    fn serialize_toml(_values: &HashMap<String, ConfigValue>) -> Result<String> {
        // Simplified TOML serialization
        // In a real implementation, you would use toml
        Ok("# TOML configuration\n".to_string())
    }

    /// Serialize to YAML
    fn serialize_yaml(_values: &HashMap<String, ConfigValue>) -> Result<String> {
        // Simplified YAML serialization
        // In a real implementation, you would use serde_yaml
        Ok("# YAML configuration\n".to_string())
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration validator
pub struct ConfigValidator {
    /// Validation rules
    rules: HashMap<String, ValidationRule>,
}

/// Validation rule
pub enum ValidationRule {
    Required,
    Range { min: i64, max: i64 },
    MinLength(usize),
    MaxLength(usize),
    Enum(Vec<String>),
    Custom(Box<dyn Fn(&ConfigValue) -> Result<()>>),
}

impl std::fmt::Debug for ValidationRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationRule::Required => write!(f, "Required"),
            ValidationRule::Range { min, max } => {
                write!(f, "Range {{ min: {}, max: {} }}", min, max)
            }
            ValidationRule::MinLength(len) => write!(f, "MinLength({})", len),
            ValidationRule::MaxLength(len) => write!(f, "MaxLength({})", len),
            ValidationRule::Enum(values) => write!(f, "Enum({:?})", values),
            ValidationRule::Custom(_) => write!(f, "Custom(<closure>)"),
        }
    }
}

impl ConfigValidator {
    /// Create a new validator
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }

    /// Add a validation rule
    pub fn add_rule(&mut self, key: &str, rule: ValidationRule) {
        self.rules.insert(key.to_string(), rule);
    }

    /// Validate configuration
    pub fn validate(&self, config: &ConfigManager) -> Result<()> {
        for (key, rule) in &self.rules {
            let value = config
                .get(key)
                .ok_or_else(|| Error::InvalidConfig(format!("Required key '{}' missing", key)))?;

            match rule {
                ValidationRule::Required => {
                    // Already checked above
                }
                ValidationRule::Range { min, max } => {
                    let int_val = value.as_integer().map_err(|_| {
                        Error::InvalidConfig(format!("'{}' must be an integer", key))
                    })?;
                    if int_val < *min || int_val > *max {
                        return Err(Error::InvalidConfig(format!(
                            "'{}' must be between {} and {}",
                            key, min, max
                        )));
                    }
                }
                ValidationRule::MinLength(min_len) => {
                    let str_val = value
                        .as_string()
                        .map_err(|_| Error::InvalidConfig(format!("'{}' must be a string", key)))?;
                    if str_val.len() < *min_len {
                        return Err(Error::InvalidConfig(format!(
                            "'{}' must be at least {} characters long",
                            key, min_len
                        )));
                    }
                }
                ValidationRule::MaxLength(max_len) => {
                    let str_val = value
                        .as_string()
                        .map_err(|_| Error::InvalidConfig(format!("'{}' must be a string", key)))?;
                    if str_val.len() > *max_len {
                        return Err(Error::InvalidConfig(format!(
                            "'{}' must be at most {} characters long",
                            key, max_len
                        )));
                    }
                }
                ValidationRule::Enum(allowed_values) => {
                    let str_val = value
                        .as_string()
                        .map_err(|_| Error::InvalidConfig(format!("'{}' must be a string", key)))?;
                    if !allowed_values.contains(&str_val) {
                        return Err(Error::InvalidConfig(format!(
                            "'{}' must be one of: {:?}",
                            key, allowed_values
                        )));
                    }
                }
                ValidationRule::Custom(validator) => {
                    validator(value)?;
                }
            }
        }

        Ok(())
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_manager() {
        let mut config = ConfigManager::new();

        config.set("test_bool", ConfigValue::Bool(true));
        config.set("test_int", ConfigValue::Integer(42));
        config.set("test_string", ConfigValue::String("hello".to_string()));

        assert_eq!(config.get("test_bool").unwrap().as_bool().unwrap(), true);
        assert_eq!(config.get("test_int").unwrap().as_integer().unwrap(), 42);
        assert_eq!(
            config.get("test_string").unwrap().as_string().unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_config_value() {
        let bool_val = ConfigValue::Bool(true);
        assert_eq!(bool_val.as_bool().unwrap(), true);

        let int_val = ConfigValue::Integer(42);
        assert_eq!(int_val.as_integer().unwrap(), 42);

        let string_val = ConfigValue::String("test".to_string());
        assert_eq!(string_val.as_string().unwrap(), "test");
    }

    #[test]
    fn test_config_validator() {
        let mut validator = ConfigValidator::new();

        validator.add_rule("test_int", ValidationRule::Range { min: 0, max: 100 });
        validator.add_rule("test_string", ValidationRule::MinLength(3));

        let mut config = ConfigManager::new();
        config.set("test_int", ConfigValue::Integer(50));
        config.set("test_string", ConfigValue::String("hello".to_string()));

        assert!(validator.validate(&config).is_ok());

        config.set("test_int", ConfigValue::Integer(150));
        assert!(validator.validate(&config).is_err());
    }
}
