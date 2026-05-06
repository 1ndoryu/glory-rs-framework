/* Parser de archivos TOML de fixtures.
 * Formato esperado:
 *   [meta]
 *   table = "users"
 *   id_field = "email"
 *   depends_on = []
 *   [meta.casts]
 *   role = "user_role"
 *
 *   [[records]]
 *   email = "test@test.com"
 *   role = "admin"
 */

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::FixtureError;

#[derive(Debug, Clone)]
pub struct FixtureMeta {
    pub table: String,
    pub id_field: String,
    /// Columna PK real de la tabla (default: "id"). Se usa para RETURNING y FK resolution.
    pub pk_field: String,
    pub depends_on: Vec<String>,
    /// Mapeo columna a tipo PG para casting (ej: role a `user_role`)
    pub casts: HashMap<String, String>,
}

#[derive(Debug)]
pub struct FixtureFile {
    pub meta: FixtureMeta,
    pub records: Vec<BTreeMap<String, toml::Value>>,
    pub source_path: PathBuf,
}

/// Lee y parsea un archivo TOML de fixtures
pub fn parse_file(path: &Path) -> Result<FixtureFile, FixtureError> {
    let content = std::fs::read_to_string(path)?;
    let doc: toml::Value = toml::from_str(&content)?;

    let meta_val = doc.get("meta").ok_or_else(|| {
        FixtureError::Validation(format!("{}: missing [meta] section", path.display()))
    })?;

    let table = meta_val
        .get("table")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            FixtureError::Validation(format!("{}: meta.table is required", path.display()))
        })?
        .to_string();

    let id_field = meta_val
        .get("id_field")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            FixtureError::Validation(format!("{}: meta.id_field is required", path.display()))
        })?
        .to_string();

    let depends_on = meta_val
        .get("depends_on")
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(toml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let pk_field = meta_val
        .get("pk_field")
        .and_then(toml::Value::as_str)
        .unwrap_or("id")
        .to_string();

    let casts = parse_casts(meta_val);

    let records_val = doc
        .get("records")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            FixtureError::Validation(format!("{}: missing [[records]] array", path.display()))
        })?;

    let mut records = Vec::with_capacity(records_val.len());
    for (idx, record_val) in records_val.iter().enumerate() {
        let table_map = record_val.as_table().ok_or_else(|| {
            FixtureError::Validation(format!("{}: record[{idx}] is not a table", path.display()))
        })?;

        if !table_map.contains_key(&id_field) {
            return Err(FixtureError::Validation(format!(
                "{}: record[{idx}] missing id_field '{id_field}'",
                path.display()
            )));
        }

        let btree: BTreeMap<String, toml::Value> = table_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        records.push(btree);
    }

    Ok(FixtureFile {
        meta: FixtureMeta {
            table,
            id_field,
            pk_field,
            depends_on,
            casts,
        },
        records,
        source_path: path.to_path_buf(),
    })
}

fn parse_casts(meta_val: &toml::Value) -> HashMap<String, String> {
    meta_val
        .get("casts")
        .and_then(toml::Value::as_table)
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}
