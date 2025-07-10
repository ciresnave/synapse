#[cfg(feature = "database")]
use sqlx::{PgPool, Row};
use anyhow::Result;
use tracing::{info, debug};

/// Database migration manager for Synapse schema
#[cfg(feature = "database")]
pub struct MigrationManager {
    pool: PgPool,
}

#[cfg(feature = "database")]
impl MigrationManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run all pending migrations
    pub async fn migrate(&self) -> Result<()> {
        info!("Starting database migrations");

        // Create migrations table if it doesn't exist
        self.create_migrations_table().await?;

        // Get current migration version
        let current_version = self.get_current_version().await?;
        debug!("Current migration version: {}", current_version);

        // Run migrations in order
        let migrations = self.get_migrations();
        for migration in migrations {
            if migration.version > current_version {
                info!("Running migration {}: {}", migration.version, migration.name);
                self.run_migration(&migration).await?;
                self.record_migration(migration.version, &migration.name).await?;
            }
        }

        info!("Database migrations completed");
        Ok(())
    }

    async fn create_migrations_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS synapse_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_current_version(&self) -> Result<i32> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(version), 0) as version FROM synapse_migrations"
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get("version"))
    }

    async fn run_migration(&self, migration: &Migration) -> Result<()> {
        sqlx::query(&migration.sql)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn record_migration(&self, version: i32, name: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO synapse_migrations (version, name) VALUES ($1, $2)"
        )
        .bind(version)
        .bind(name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    fn get_migrations(&self) -> Vec<Migration> {
        vec![
            Migration {
                version: 1,
                name: "Create initial Synapse schema".to_string(),
                sql: include_str!("../../../migrations/001_create_synapse_schema.sql").to_string(),
            },
            // Add more migrations here as needed
        ]
    }
}

#[derive(Debug)]
struct Migration {
    version: i32,
    name: String,
    sql: String,
}
