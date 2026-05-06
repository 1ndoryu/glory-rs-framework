/* sentinel-disable-file sqlx-query-sin-macro sqlx-query-as-sin-macro: fixtures usan runtime
 * queries porque construyen SQL dinámico basado en esquemas TOML genéricos. */
/* Glory Fixtures — Sistema declarativo de content fixtures para PostgreSQL.
 * Lee archivos TOML desde un directorio content/, sincroniza con la BD
 * (insert/update/delete), y rastrea qué registros son gestionados via
 * tabla _glory_fixtures. Agnóstico del proyecto — funciona con cualquier tabla.
 * Password hashing se delega al proyecto via callback (no trae argon2). */

mod parser;
mod sync;

pub use parser::{FixtureFile, FixtureMeta};

use sqlx::PgPool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("Database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Validation: {0}")]
    Validation(String),
}

/// Callback para hashear passwords cuando se encuentra "plain:xxx" en un valor.
/// El proyecto pasa su propia función (argon2, bcrypt, etc.).
pub type PasswordHasher =
    Box<dyn Fn(&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

#[derive(Debug, Default)]
pub struct SyncReport {
    pub inserted: u64,
    pub updated: u64,
    pub deleted: u64,
    pub skipped: u64,
    pub errors: Vec<String>,
}

impl SyncReport {
    pub fn merge(&mut self, other: &SyncReport) {
        self.inserted += other.inserted;
        self.updated += other.updated;
        self.deleted += other.deleted;
        self.skipped += other.skipped;
        self.errors.extend(other.errors.iter().cloned());
    }

    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "inserted={} updated={} deleted={} skipped={} errors={}",
            self.inserted,
            self.updated,
            self.deleted,
            self.skipped,
            self.errors.len()
        )
    }
}

pub struct ContentManager {
    pool: PgPool,
    content_dir: PathBuf,
    password_hasher: Option<PasswordHasher>,
}

impl ContentManager {
    #[must_use]
    pub fn new(pool: PgPool, content_dir: impl AsRef<Path>) -> Self {
        Self {
            pool,
            content_dir: content_dir.as_ref().to_path_buf(),
            password_hasher: None,
        }
    }

    /// Registra un callback para hashear passwords cuando se encuentra "plain:xxx"
    #[must_use]
    pub fn with_password_hasher(mut self, hasher: PasswordHasher) -> Self {
        self.password_hasher = Some(hasher);
        self
    }

    /// Crea la tabla `_glory_fixtures` si no existe
    pub async fn ensure_tracking_table(&self) -> Result<(), FixtureError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _glory_fixtures (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                fixture_file TEXT NOT NULL,
                table_name TEXT NOT NULL,
                id_field TEXT NOT NULL,
                record_id TEXT NOT NULL,
                db_id TEXT,
                content_hash TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                updated_at TIMESTAMPTZ DEFAULT NOW(),
                UNIQUE(table_name, record_id)
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Sincroniza todos los archivos TOML del directorio content/
    pub async fn sync_all(&self) -> Result<SyncReport, FixtureError> {
        self.ensure_tracking_table().await?;

        let paths = self.discover_fixtures()?;
        if paths.is_empty() {
            tracing::info!(
                "[fixtures] No fixture files found in {}",
                self.content_dir.display()
            );
            return Ok(SyncReport::default());
        }

        let mut fixtures = Vec::new();
        for path in &paths {
            match parser::parse_file(path) {
                Ok(f) => fixtures.push(f),
                Err(e) => tracing::error!("[fixtures] Error parsing {}: {e}", path.display()),
            }
        }

        let sorted = topological_sort(&fixtures);

        let mut report = SyncReport::default();
        for fixture in &sorted {
            match sync::sync_fixture(&self.pool, fixture, self.password_hasher.as_deref()).await {
                Ok(r) => report.merge(&r),
                Err(e) => report.errors.push(format!("{}: {e}", fixture.meta.table)),
            }
        }

        /* Fase 3: orphan cleanup — borrar registros tracked que ya no están en fixtures */
        match self.clean_orphans_internal(&sorted).await {
            Ok(n) => report.deleted += n,
            Err(e) => report.errors.push(format!("orphan cleanup: {e}")),
        }

        for err in &report.errors {
            tracing::error!("[fixtures] {err}");
        }

        tracing::info!("[fixtures] Sync complete: {}", report.summary());
        Ok(report)
    }

    /// Sincroniza un solo archivo de fixture
    pub async fn sync_file(&self, path: &Path) -> Result<SyncReport, FixtureError> {
        self.ensure_tracking_table().await?;
        let fixture = parser::parse_file(path)?;
        sync::sync_fixture(&self.pool, &fixture, self.password_hasher.as_deref()).await
    }

    /// Elimina registros tracked que ya no están en ningún fixture
    pub async fn clean_orphans(&self) -> Result<u64, FixtureError> {
        self.ensure_tracking_table().await?;
        let paths = self.discover_fixtures()?;
        let mut fixtures = Vec::new();
        for path in &paths {
            if let Ok(f) = parser::parse_file(path) {
                fixtures.push(f);
            }
        }
        let refs: Vec<&FixtureFile> = fixtures.iter().collect();
        self.clean_orphans_internal(&refs).await
    }

    fn discover_fixtures(&self) -> Result<Vec<PathBuf>, FixtureError> {
        if !self.content_dir.exists() {
            return Ok(vec![]);
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&self.content_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    async fn clean_orphans_internal(&self, fixtures: &[&FixtureFile]) -> Result<u64, FixtureError> {
        let tracked: Vec<TrackedRecord> =
            sqlx::query_as("SELECT table_name, id_field, record_id FROM _glory_fixtures")
                .fetch_all(&self.pool)
                .await?;

        if tracked.is_empty() {
            return Ok(0);
        }

        /* Construir set de (table, record_id) que existen actualmente en fixtures */
        let mut current: HashSet<(String, String)> = HashSet::new();
        for f in fixtures {
            for record in &f.records {
                if let Some(id_val) = record.get(&f.meta.id_field) {
                    current.insert((f.meta.table.clone(), sync::toml_value_to_id_string(id_val)));
                }
            }
        }

        let mut deleted = 0u64;
        for tr in &tracked {
            if current.contains(&(tr.table_name.clone(), tr.record_id.clone())) {
                continue;
            }
            /* Registro huérfano: borrar de la tabla real y del tracking */
            let id_col = sync::sanitize_identifier(&tr.id_field)?;
            let table = sync::sanitize_identifier(&tr.table_name)?;

            let delete_sql = format!("DELETE FROM {table} WHERE {id_col} = $1");
            if let Err(e) = sqlx::query(&delete_sql)
                .bind(&tr.record_id)
                .execute(&self.pool)
                .await
            {
                tracing::warn!(
                    "[fixtures] Failed to delete orphan {}.{}={}: {e}",
                    tr.table_name,
                    tr.id_field,
                    tr.record_id
                );
                continue;
            }

            sqlx::query("DELETE FROM _glory_fixtures WHERE table_name = $1 AND record_id = $2")
                .bind(&tr.table_name)
                .bind(&tr.record_id)
                .execute(&self.pool)
                .await?;

            tracing::info!(
                "[fixtures] Deleted orphan: {}.{}={}",
                tr.table_name,
                tr.id_field,
                tr.record_id
            );
            deleted += 1;
        }

        Ok(deleted)
    }
}

#[derive(sqlx::FromRow)]
struct TrackedRecord {
    table_name: String,
    id_field: String,
    record_id: String,
}

/// Ordenamiento topológico simple por `depends_on`
fn topological_sort(fixtures: &[FixtureFile]) -> Vec<&FixtureFile> {
    let mut sorted: Vec<&FixtureFile> = Vec::new();
    let mut remaining: Vec<&FixtureFile> = fixtures.iter().collect();
    let max_iter = remaining
        .len()
        .saturating_mul(remaining.len())
        .saturating_add(1);
    let mut iterations = 0;

    while !remaining.is_empty() && iterations < max_iter {
        let prev_len = remaining.len();
        remaining.retain(|f| {
            let satisfied = f
                .meta
                .depends_on
                .iter()
                .all(|dep| sorted.iter().any(|s| s.meta.table == *dep));
            if satisfied {
                sorted.push(f);
                false
            } else {
                true
            }
        });
        if remaining.len() == prev_len {
            tracing::warn!(
                "[fixtures] Circular/missing dependency, adding remaining as-is: {:?}",
                remaining.iter().map(|f| &f.meta.table).collect::<Vec<_>>()
            );
            sorted.append(&mut remaining);
            break;
        }
        iterations += 1;
    }

    sorted
}
