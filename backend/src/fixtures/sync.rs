/* sentinel-disable-file sqlx-query-sin-macro sqlx-query-as-sin-macro
 * Justificación: sync.rs genera queries dinámicas a partir de definiciones TOML.
 * Las tablas/columnas se validan con whitelist alfanumérica, pero los queries
 * no son estáticos — no pueden usar macros sqlx. */
/* Lógica de sincronización: diff hash → UPSERT → tracking.
 * Usa PgArguments para binding dinámico de tipos según el valor TOML.
 * Seguridad SQL: todos los identificadores (tabla, columna) se validan
 * con whitelist alfanumérica + underscore antes de interpolar. */

use sha2::{Digest, Sha256};
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, Encode, PgPool, Postgres, Type};
use std::collections::BTreeMap;

use super::parser::FixtureFile;
use super::{FixtureError, SyncReport};

/// Tipo para el callback de password hashing
type HasherFn =
    dyn Fn(&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> + Send + Sync;

/* [104A-10] Un hash idéntico en _glory_fixtures no garantiza que el registro siga
 * existiendo en la tabla real. Si alguien borró un fixture-managed row manualmente,
 * el próximo sync debe reinsertarlo en vez de saltarlo para no dejar seeds rotos. */
async fn tracked_record_exists(
    pool: &PgPool,
    safe_table: &str,
    safe_pk_col: &str,
    tracked_db_id: Option<&str>,
) -> Result<bool, FixtureError> {
    let Some(tracked_db_id) = tracked_db_id else {
        return Ok(false);
    };

    let sql = format!("SELECT EXISTS(SELECT 1 FROM {safe_table} WHERE {safe_pk_col}::text = $1)");

    Ok(sqlx::query_scalar(&sql)
        .bind(tracked_db_id)
        .fetch_one(pool)
        .await?)
}

/// Sincroniza un fixture completo contra la BD
pub async fn sync_fixture(
    pool: &PgPool,
    fixture: &FixtureFile,
    password_hasher: Option<&HasherFn>,
) -> Result<SyncReport, FixtureError> {
    let table = sanitize_identifier(&fixture.meta.table)?;
    let id_col = sanitize_identifier(&fixture.meta.id_field)?;
    let pk_col = sanitize_identifier(&fixture.meta.pk_field)?;
    let file_name = fixture
        .source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let mut report = SyncReport::default();

    for record in &fixture.records {
        let id_raw = record.get(&fixture.meta.id_field).ok_or_else(|| {
            FixtureError::Validation(format!(
                "Record missing id_field '{}'",
                fixture.meta.id_field
            ))
        })?;
        let record_id = toml_value_to_id_string(id_raw);

        /* Computar hash del contenido para detectar cambios */
        let hash = compute_hash(record);

        /* Verificar si ya está tracked con el mismo hash */
        let existing_tracking: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT content_hash, db_id FROM _glory_fixtures WHERE table_name = $1 AND record_id = $2",
        )
        .bind(&fixture.meta.table)
        .bind(&record_id)
        .fetch_optional(pool)
        .await?;

        if let Some((existing_hash, tracked_db_id)) = existing_tracking.as_ref() {
            let row_still_exists =
                tracked_record_exists(pool, &table, &pk_col, tracked_db_id.as_deref()).await?;

            if existing_hash == &hash && row_still_exists {
                report.skipped += 1;
                continue;
            }
        }

        /* Fase 4: resolver FK references (@tabla:id → db_id real) */
        let resolved = resolve_fk_references(pool, record).await?;

        /* Procesar valores: resolver plain:password, convertir tipos */
        let processed = process_record(&resolved, password_hasher)?;

        /* Construir y ejecutar UPSERT con RETURNING pk_field */
        let columns: Vec<&str> = processed.keys().map(String::as_str).collect();
        let upsert_sql = build_upsert_sql(
            &table,
            &id_col,
            &fixture.meta.id_field,
            &pk_col,
            &columns,
            &fixture.meta.casts,
        )?;

        let args = build_arguments(&columns, &processed)?;
        let db_id: String = sqlx::query_scalar_with(&upsert_sql, args)
            .fetch_one(pool)
            .await
            .map_err(|e| {
                FixtureError::Validation(format!("{}.{record_id}: {e}", fixture.meta.table))
            })?;

        /* Actualizar tracking con db_id para FK resolution futura */
        sqlx::query(
            "INSERT INTO _glory_fixtures (fixture_file, table_name, id_field, record_id, db_id, content_hash)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (table_name, record_id)
             DO UPDATE SET content_hash = $6, db_id = $5, fixture_file = $1, updated_at = NOW()",
        )
        .bind(file_name)
        .bind(&fixture.meta.table)
        .bind(&fixture.meta.id_field)
        .bind(&record_id)
        .bind(&db_id)
        .bind(&hash)
        .execute(pool)
        .await?;

        if existing_tracking.is_some() {
            report.updated += 1;
            tracing::debug!("[fixtures] Updated {}.{record_id}", fixture.meta.table);
        } else {
            report.inserted += 1;
            tracing::debug!("[fixtures] Inserted {}.{record_id}", fixture.meta.table);
        }
    }

    Ok(report)
}

/// Valida que un identificador SQL solo contenga [a-zA-Z0-9_] y lo envuelve en comillas
pub fn sanitize_identifier(name: &str) -> Result<String, FixtureError> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(FixtureError::Validation(format!(
            "Invalid SQL identifier: '{name}'"
        )));
    }
    Ok(format!("\"{name}\""))
}

/// Convierte un valor TOML a string para usar como `record_id`
pub fn toml_value_to_id_string(val: &toml::Value) -> String {
    match val {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(n) => n.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}

/* Enum para valores procesados que se pueden bindear a PgArguments.
 * Cada variante corresponde a un tipo que SQLx sabe encodear. */
#[derive(Debug, Clone)]
enum Processed {
    Text(String),
    Int64(i64),
    Float64(f64),
    Bool(bool),
    Json(serde_json::Value),
    /// UUID resuelto desde FK reference — se bindea como `uuid::Uuid`
    Uuid(uuid::Uuid),
}

fn compute_hash(record: &BTreeMap<String, toml::Value>) -> String {
    let serialized = format!("{record:?}");
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let result = hasher.finalize();
    /* Hex encoding manual — evita dependencia de hex crate */
    let mut hex = String::with_capacity(result.len() * 2);
    for byte in &result {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/* Fase 4: Resuelve FK references en un record.
 * Sintaxis @tabla:record_id → db_id desde _glory_fixtures (tabla ya sincronizada).
 * Sintaxis @lookup:tabla:col=val[&col2=val2] → SELECT id FROM tabla WHERE col = val.
 * Los valores en @lookup pueden ser @tabla:id (refs anidadas, se resuelven primero).
 * Ejemplo: plan_id = "@lookup:service_plans:slug=basico&service_id=@services:diseno-web" */
async fn resolve_fk_references(
    pool: &PgPool,
    record: &BTreeMap<String, toml::Value>,
) -> Result<BTreeMap<String, toml::Value>, FixtureError> {
    let mut resolved = BTreeMap::new();

    for (key, val) in record {
        if let toml::Value::String(s) = val {
            if let Some(reference) = s.strip_prefix('@') {
                let db_id = resolve_single_reference(pool, reference, key).await?;
                resolved.insert(key.clone(), toml::Value::String(db_id));
                continue;
            }
        }
        resolved.insert(key.clone(), val.clone());
    }

    Ok(resolved)
}

/* Resuelve una referencia individual (sin el @).
 * "tabla:record_id" → lookup en _glory_fixtures.
 * "lookup:tabla:col=val&col2=val2" → SELECT directo en la tabla real.
 * Usa Box::pin por recursión async (lookup puede contener refs anidadas). */
fn resolve_single_reference<'a>(
    pool: &'a PgPool,
    reference: &'a str,
    field_name: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, FixtureError>> + Send + 'a>>
{
    Box::pin(async move {
        if let Some(lookup_body) = reference.strip_prefix("lookup:") {
            return resolve_lookup(pool, lookup_body, field_name).await;
        }

        /* Referencia estándar: tabla:record_id → _glory_fixtures */
        let (ref_table, ref_id) = reference.split_once(':').ok_or_else(|| {
            FixtureError::Validation(format!(
                "Invalid FK reference '@{reference}' in '{field_name}'. Expected @table:record_id"
            ))
        })?;

        let db_id: Option<String> = sqlx::query_scalar(
            "SELECT db_id FROM _glory_fixtures WHERE table_name = $1 AND record_id = $2",
        )
        .bind(ref_table)
        .bind(ref_id)
        .fetch_optional(pool)
        .await?;

        db_id.ok_or_else(|| {
            FixtureError::Validation(format!(
                "FK reference @{ref_table}:{ref_id} not found in tracking table. \
                 Is '{ref_table}' listed in depends_on?"
            ))
        })
    })
}

/* Resuelve @lookup:tabla:col=val[&col2=val2].
 * Cada valor puede ser otra referencia @tabla:id (se resuelve recursivamente).
 * Construye SELECT "id" FROM tabla WHERE col1 = $1 AND col2 = $2 LIMIT 1.
 * Todos los identificadores se validan con sanitize_identifier. */
async fn resolve_lookup(
    pool: &PgPool,
    body: &str,
    field_name: &str,
) -> Result<String, FixtureError> {
    /* Separar tabla de las condiciones: "service_plans:slug=basico&service_id=UUID" */
    let (table_name, conditions_str) = body.split_once(':').ok_or_else(|| {
        FixtureError::Validation(format!(
            "Invalid @lookup in '{field_name}': expected @lookup:table:col=val"
        ))
    })?;

    let safe_table = sanitize_identifier(table_name)?;

    /* Parsear condiciones separadas por & */
    let mut where_parts: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();

    for (i, pair) in conditions_str.split('&').enumerate() {
        let (col, raw_val) = pair.split_once('=').ok_or_else(|| {
            FixtureError::Validation(format!(
                "Invalid @lookup condition '{pair}' in '{field_name}'. Expected col=value"
            ))
        })?;

        let safe_col = sanitize_identifier(col)?;

        /* Si el valor es otra referencia @tabla:id, resolverla primero */
        let resolved_val = if let Some(nested_ref) = raw_val.strip_prefix('@') {
            resolve_single_reference(pool, nested_ref, field_name).await?
        } else {
            raw_val.to_string()
        };

        /* Si el valor resuelto parece UUID, castear el parámetro para que PG compare correctamente */
        let is_uuid = uuid::Uuid::parse_str(&resolved_val).is_ok();
        let param = format!("${}", i + 1);
        let casted_param = if is_uuid {
            format!("{param}::uuid")
        } else {
            param
        };
        where_parts.push(format!("{safe_col} = {casted_param}"));
        bind_values.push(resolved_val);
    }

    if where_parts.is_empty() {
        return Err(FixtureError::Validation(format!(
            "@lookup in '{field_name}' has no conditions"
        )));
    }

    let sql = format!(
        "SELECT \"id\"::text FROM {safe_table} WHERE {} LIMIT 1",
        where_parts.join(" AND ")
    );

    /* Ejecutar con all bind values como strings (PG hará cast implícito para UUID) */
    let mut query = sqlx::query_scalar::<_, String>(&sql);
    for val in &bind_values {
        query = query.bind(val);
    }

    let result = query.fetch_optional(pool).await?;

    result.ok_or_else(|| {
        let pairs: Vec<String> = conditions_str.split('&').map(String::from).collect();
        FixtureError::Validation(format!(
            "@lookup:{table_name} not found for conditions [{}] in '{field_name}'",
            pairs.join(", ")
        ))
    })
}

fn process_record(
    record: &BTreeMap<String, toml::Value>,
    password_hasher: Option<&HasherFn>,
) -> Result<BTreeMap<String, Processed>, FixtureError> {
    let mut out = BTreeMap::new();

    for (key, val) in record {
        let processed = match val {
            toml::Value::String(s) => {
                if let Some(plain) = s.strip_prefix("plain:") {
                    let hasher = password_hasher.ok_or_else(|| {
                        FixtureError::Validation(
                            "Found 'plain:' password but no password_hasher was provided".into(),
                        )
                    })?;
                    let password_hash = hasher(plain).map_err(|e| {
                        FixtureError::Validation(format!("Password hash error for '{key}': {e}"))
                    })?;
                    Processed::Text(password_hash)
                } else if let Ok(uuid_val) = uuid::Uuid::parse_str(s) {
                    /* UUID strings (resolved FK refs) se bindean como Uuid
                     * para que PG no necesite cast explícito */
                    Processed::Uuid(uuid_val)
                } else {
                    Processed::Text(s.clone())
                }
            }
            toml::Value::Integer(n) => Processed::Int64(*n),
            toml::Value::Float(f) => Processed::Float64(*f),
            toml::Value::Boolean(b) => Processed::Bool(*b),
            toml::Value::Array(_) | toml::Value::Table(_) => Processed::Json(toml_to_json(val)),
            toml::Value::Datetime(d) => Processed::Text(d.to_string()),
        };
        out.insert(key.clone(), processed);
    }

    Ok(out)
}

fn toml_to_json(val: &toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(n) => serde_json::json!(n),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::json!(b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let map: serde_json::Map<String, serde_json::Value> = t
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

/* Genera SQL tipo:
 * INSERT INTO "table" ("col1", "col2") VALUES ($1, $2::cast_type)
 * ON CONFLICT ("id_field") DO UPDATE SET "col1" = EXCLUDED."col1", ...
 * RETURNING "pk_field"::text
 *
 * Usa EXCLUDED para el SET — más limpio que repetir $N.
 * RETURNING permite capturar el PK real para FK resolution. */
fn build_upsert_sql(
    table: &str,
    id_col: &str,
    id_field_raw: &str,
    pk_col: &str,
    columns: &[&str],
    casts: &std::collections::HashMap<String, String>,
) -> Result<String, FixtureError> {
    let col_list: Vec<String> = columns
        .iter()
        .map(|c| sanitize_identifier(c))
        .collect::<Result<Vec<_>, _>>()?;

    let placeholders: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let param = format!("${}", i + 1);
            if let Some(cast) = casts.get(*col) {
                /* Validar el cast type para prevenir inyección */
                if cast.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    format!("{param}::{cast}")
                } else {
                    param
                }
            } else {
                param
            }
        })
        .collect();

    let set_clause: Vec<String> = columns
        .iter()
        .filter(|c| **c != id_field_raw)
        .map(|c| {
            let safe = sanitize_identifier(c).unwrap_or_default();
            format!("{safe} = EXCLUDED.{safe}")
        })
        .collect();

    let returning = format!("RETURNING {pk_col}::text");

    if set_clause.is_empty() {
        /* Solo hay id_field — DO NOTHING no soporta RETURNING si no hubo insert.
         * Usamos DO UPDATE SET id = EXCLUDED.id como no-op para forzar RETURNING. */
        return Ok(format!(
            "INSERT INTO {table} ({cols}) VALUES ({vals}) ON CONFLICT ({id_col}) DO UPDATE SET {id_col} = EXCLUDED.{id_col} {returning}",
            cols = col_list.join(", "),
            vals = placeholders.join(", "),
        ));
    }

    Ok(format!(
        "INSERT INTO {table} ({cols}) VALUES ({vals}) ON CONFLICT ({id_col}) DO UPDATE SET {sets} {returning}",
        cols = col_list.join(", "),
        vals = placeholders.join(", "),
        sets = set_clause.join(", "),
    ))
}

/* Construye PgArguments con los valores procesados en el orden de columns.
 * Cada tipo Processed se bindea como su tipo Rust correspondiente. */
fn build_arguments(
    columns: &[&str],
    processed: &BTreeMap<String, Processed>,
) -> Result<PgArguments, FixtureError> {
    let mut args = PgArguments::default();

    for col in columns {
        let val = processed.get(*col).ok_or_else(|| {
            FixtureError::Validation(format!("Missing processed value for column '{col}'"))
        })?;

        match val {
            Processed::Text(s) => bind_arg(&mut args, s.clone())?,
            Processed::Int64(n) => bind_arg(&mut args, *n)?,
            Processed::Float64(f) => bind_arg(&mut args, *f)?,
            Processed::Bool(b) => bind_arg(&mut args, *b)?,
            Processed::Json(j) => bind_arg(&mut args, j.clone())?,
            Processed::Uuid(u) => bind_arg(&mut args, *u)?,
        }
    }

    Ok(args)
}

fn bind_arg<'q, T>(args: &mut PgArguments, value: T) -> Result<(), FixtureError>
where
    T: Encode<'q, Postgres> + Type<Postgres> + 'q,
{
    args.add(value)
        .map_err(|e| FixtureError::Validation(format!("Bind error: {e}")))
}
