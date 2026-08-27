use std::collections::HashMap;

use crate::db::DbError;

pub fn parse_key_value(value: &str) -> Result<HashMap<String, String>, DbError> {
    let mut result = HashMap::new();
    for part in value.split(';').filter(|part| !part.trim().is_empty()) {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            DbError::Connection(format!("Invalid connection-string entry: {part}"))
        })?;
        result.insert(
            key.trim().to_ascii_lowercase(),
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        );
    }
    Ok(result)
}

pub fn first_value(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| values.get(*key).cloned())
}
