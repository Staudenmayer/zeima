//! SQLite persistence: connection setup, bootstrap and the write
//! transaction that keeps the change log in sync with every write.

use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::Serialize;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{Row, Sqlite, Transaction};

use crate::clock::Clock;
use crate::events::DomainEvent;
use crate::hlc::HlcClock;
use crate::ids::new_id;
use crate::Result;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Who and where this install is. Created once on first start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub device_id: String,
    pub workspace_id: String,
    pub user_id: String,
    /// An organization's shared workspace: `user_id` is the Keycloak account
    /// and is part of the synced rows (the server checks who owns what).
    /// In the personal workspace it is a local id that never leaves the device.
    pub shared: bool,
    /// Acting for another person of the organization (see `Core::managing`): the one case in which a shared handle
    /// may set a person's schedule, vacation entitlement and overtime corrections.
    pub managing: bool,
}

/// A workspace on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: String,
    /// `personal` or `org`.
    pub kind: String,
    pub org_id: Option<String>,
    pub name: String,
}

/// Entry point for all reads and writes. Every write goes through
/// [`Core::begin`] so it is stamped and written to the change log.
pub struct Core {
    pool: SqlitePool,
    ctx: Context,
    hlc: Arc<Mutex<HlcClock>>,
    clock: Arc<dyn Clock>,
    default_tz: Arc<RwLock<String>>,
    week_start: Arc<RwLock<crate::locale::WeekStart>>,
    events: tokio::sync::broadcast::Sender<DomainEvent>,
}

impl Core {
    /// Opens (or creates) the database file, runs migrations and bootstraps
    /// the local workspace and user on first start.
    pub async fn open(path: &Path, clock: Arc<dyn Clock>) -> Result<Self> {
        // The folder is the owner's alone (Unix), and so is the file with everything in it.
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            crate::fsperm::private_dir(parent).map_err(sqlx::Error::Io)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        // The write-ahead files exist now, too.
        for suffix in ["", "-wal", "-shm"] {
            let mut file = path.as_os_str().to_owned();
            file.push(suffix);
            let _ = crate::fsperm::private_file(Path::new(&file));
        }
        Self::init(pool, clock).await
    }

    /// In-memory database for tests. A single connection, because every
    /// in-memory connection is its own database.
    pub async fn open_in_memory(clock: Arc<dyn Clock>) -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::init(pool, clock).await
    }

    async fn init(pool: SqlitePool, clock: Arc<dyn Clock>) -> Result<Self> {
        MIGRATOR.run(&pool).await?;
        let device_id = match get_meta(&pool, "device_id").await? {
            Some(id) => id,
            None => {
                let id = new_id();
                set_meta(&pool, "device_id", &id).await?;
                id
            }
        };
        let mut core = Self {
            pool,
            ctx: Context {
                device_id: device_id.clone(),
                workspace_id: String::new(),
                user_id: String::new(),
                shared: false,
                managing: false,
            },
            hlc: Arc::new(Mutex::new(HlcClock::new(device_id))),
            clock,
            default_tz: Arc::new(RwLock::new("UTC".into())),
            week_start: Arc::new(RwLock::new(crate::locale::WeekStart::Monday)),
            events: tokio::sync::broadcast::channel(256).0,
        };
        core.bootstrap_identity().await?;
        core.ensure_absence_types().await?;
        Ok(core)
    }

    /// Creates the personal workspace and local user on first start.
    async fn bootstrap_identity(&mut self) -> Result<()> {
        let workspace_id = get_meta(&self.pool, "workspace_id").await?;
        let user_id = get_meta(&self.pool, "user_id").await?;
        if let (Some(workspace_id), Some(user_id)) = (workspace_id, user_id) {
            self.ctx.workspace_id = workspace_id;
            self.ctx.user_id = user_id;
            return Ok(());
        }

        let workspace_id = new_id();
        let user_id = new_id();
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO workspace (id, kind, name, created_at, updated_at, hlc, origin_device)
             VALUES (?, 'personal', 'Personal', ?, ?, ?, ?)",
        )
        .bind(&workspace_id)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert(
            "workspace",
            &workspace_id,
            &s,
            &serde_json::json!({ "id": workspace_id, "kind": "personal", "name": "Personal" }),
        )
        .await?;

        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO app_user (id, display_name, created_at, updated_at, hlc, origin_device)
             VALUES (?, 'Me', ?, ?, ?, ?)",
        )
        .bind(&user_id)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert(
            "app_user",
            &user_id,
            &s,
            &serde_json::json!({ "id": user_id, "displayName": "Me" }),
        )
        .await?;

        set_meta(&mut *tx.conn(), "workspace_id", &workspace_id).await?;
        set_meta(&mut *tx.conn(), "user_id", &user_id).await?;
        tx.commit().await?;

        self.ctx.workspace_id = workspace_id;
        self.ctx.user_id = user_id;
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn ctx(&self) -> &Context {
        &self.ctx
    }

    /// The same database, clock, settings and event stream, acting in
    /// another workspace as another user (an organization's shared
    /// workspace, where the user is the Keycloak account). Everything the
    /// scoped handle reads or writes belongs to that workspace.
    pub fn scoped(&self, workspace_id: &str, user_id: &str) -> Core {
        Core {
            pool: self.pool.clone(),
            ctx: Context {
                device_id: self.ctx.device_id.clone(),
                workspace_id: workspace_id.to_string(),
                user_id: user_id.to_string(),
                shared: true,
                managing: false,
            },
            hlc: self.hlc.clone(),
            clock: self.clock.clone(),
            default_tz: self.default_tz.clone(),
            week_start: self.week_start.clone(),
            events: self.events.clone(),
        }
    }

    /// A handle that sets the schedule, vacation entitlement and overtime corrections of `user_id` in this
    /// organization workspace. Those are the organization's records about a person: on a plain shared handle they
    /// cannot be changed (the server refuses them from anyone without the right to manage members). Only meaningful
    /// in a shared workspace; elsewhere it is the handle itself.
    pub fn managing(&self, user_id: &str) -> Core {
        let mut core = self.scoped(&self.ctx.workspace_id, user_id);
        core.ctx.shared = self.ctx.shared;
        core.ctx.managing = self.ctx.shared;
        core
    }

    /// Refuses a change to what the organization sets for a person, unless this is a `managing` handle.
    pub(crate) fn require_own_settings(&self) -> Result<()> {
        if self.ctx.shared && !self.ctx.managing {
            return Err(crate::Error::validation(
                "workspace",
                "this is set by an administrator of the organization",
            ));
        }
        Ok(())
    }

    /// Records an organization's shared workspace and the user's account in
    /// it, as the server reported them. Not written to the change log: they
    /// come from the server and are not sent back. Idempotent.
    pub async fn adopt_org_workspace(
        &self,
        workspace_id: &str,
        org_id: &str,
        name: &str,
        user_id: &str,
        display_name: &str,
    ) -> Result<()> {
        let now = self.clock.now_ms();
        let hlc = format!("{now:015}-000000-server");
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO workspace (id, kind, org_id, name, created_at, updated_at, hlc, origin_device)
             VALUES (?, 'org', ?, ?, ?, ?, ?, 'server')
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, org_id = excluded.org_id, deleted_at = NULL",
        )
        .bind(workspace_id)
        .bind(org_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .bind(&hlc)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO app_user (id, display_name, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, 'server')
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .bind(now)
        .bind(&hlc)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Removes an organization's shared workspace from this device, with everything in it: entries, pauses,
    /// projects, tags, every other per-workspace table, the change log and what is remembered of the server's
    /// state for those rows. Used when the server no longer lists the workspace (the person left or was removed).
    /// Refuses the personal workspace. Returns whether a workspace was removed. All or nothing.
    pub async fn remove_org_workspace(&self, workspace_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM workspace WHERE id = ?")
            .bind(workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
        if kind.as_deref() != Some("org") {
            return Ok(false);
        }
        // Rows refer to each other (pauses to entries, entries to projects); check only at the end.
        sqlx::query("PRAGMA defer_foreign_keys = ON").execute(&mut *tx).await?;
        let tables: Vec<(String, i64)> = sqlx::query_as(
            "SELECT m.name, EXISTS (SELECT 1 FROM pragma_table_info(m.name) WHERE name = 'id')
             FROM sqlite_master m
             WHERE m.type = 'table' AND m.name NOT IN ('workspace', 'change_log')
               AND EXISTS (SELECT 1 FROM pragma_table_info(m.name) WHERE name = 'workspace_id')",
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM time_entry_tag WHERE entry_id IN (SELECT id FROM time_entry WHERE workspace_id = ?)")
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;
        for (table, has_id) in &tables {
            if *has_id != 0 {
                for state in ["sync_state", "sync_conflict"] {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {state} WHERE table_name = ?1 AND row_id IN (SELECT id FROM \"{table}\" WHERE workspace_id = ?2)"
                    )))
                    .bind(table)
                    .bind(workspace_id)
                    .execute(&mut *tx)
                    .await?;
                }
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "DELETE FROM sync_alias WHERE table_name = ?1 AND local_id IN (SELECT id FROM \"{table}\" WHERE workspace_id = ?2)"
                )))
                .bind(table)
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            }
        }
        for (table, _) in &tables {
            sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM \"{table}\" WHERE workspace_id = ?")))
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM change_log WHERE workspace_id = ?")
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM workspace WHERE id = ?")
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// The workspaces on this device: the personal one and organizations'.
    pub async fn workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        let rows = sqlx::query(
            "SELECT id, kind, org_id, name FROM workspace WHERE deleted_at IS NULL
             ORDER BY (kind = 'personal') DESC, name COLLATE NOCASE, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(WorkspaceInfo {
                    id: r.try_get("id")?,
                    kind: r.try_get("kind")?,
                    org_id: r.try_get("org_id")?,
                    name: r.try_get("name")?,
                })
            })
            .collect()
    }

    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Moves the hybrid clock past a timestamp received from another device,
    /// so later local writes sort after it.
    pub(crate) fn observe_hlc(&self, remote: &str) {
        let now = self.clock.now_ms();
        let _ = self
            .hlc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(remote, now);
    }

    /// IANA zone stamped on new entries when none is given (the OS zone).
    pub fn default_tz(&self) -> String {
        self.default_tz.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_default_tz(&self, tz: &str) -> Result<()> {
        crate::model::validate_tz(tz)?;
        *self.default_tz.write().unwrap_or_else(|e| e.into_inner()) = tz.to_string();
        Ok(())
    }

    /// First day of the week for weekly budgets and report weeks.
    pub fn week_start(&self) -> crate::locale::WeekStart {
        *self.week_start.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_week_start(&self, week_start: crate::locale::WeekStart) {
        *self.week_start.write().unwrap_or_else(|e| e.into_inner()) = week_start;
    }

    /// Drops all cached days, e.g. after the time zone changed (day
    /// boundaries move). They are recomputed on demand.
    pub async fn clear_day_cache(&self) -> Result<()> {
        sqlx::query("DELETE FROM daily_summary")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn begin(&self) -> Result<WriteTx<'_>> {
        Ok(WriteTx {
            tx: self.pool.begin().await?,
            core: self,
            now: self.clock.now_ms(),
            events: Vec::new(),
        })
    }

    /// Domain events of committed writes. A slow listener that falls more
    /// than 256 events behind skips the oldest ones.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.events.subscribe()
    }

    /// Publishes an event that does not come from a write (e.g. `day.closed`).
    pub fn publish(&self, event: DomainEvent) {
        // No listeners is fine.
        let _ = self.events.send(event);
    }

    pub async fn get_meta(&self, key: &str) -> Result<Option<String>> {
        get_meta(&self.pool, key).await
    }

    pub async fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        set_meta(&self.pool, key, value).await
    }

    /// Number of changes not yet pushed to a sync server.
    pub async fn pending_changes(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM change_log WHERE pushed_at IS NULL")
                .fetch_one(&self.pool)
                .await?,
        )
    }
}

async fn get_meta<'e, E>(executor: E, key: &str) -> Result<Option<String>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_scalar("SELECT value FROM local_meta WHERE key = ?")
        .bind(key)
        .fetch_optional(executor)
        .await?)
}

async fn set_meta<'e, E>(executor: E, key: &str, value: &str) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO local_meta (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(executor)
    .await?;
    Ok(())
}

/// Values for the common columns of a row being written.
#[derive(Debug, Clone)]
pub struct Stamp {
    pub now: i64,
    pub hlc: String,
    pub device: String,
}

/// A write transaction. Repositories write their rows through
/// [`WriteTx::conn`] and then record the change with [`WriteTx::log_upsert`]
/// or [`WriteTx::log_delete`]; both land in the same transaction.
pub struct WriteTx<'c> {
    tx: Transaction<'static, Sqlite>,
    core: &'c Core,
    now: i64,
    /// Published after a successful commit.
    events: Vec<DomainEvent>,
}

impl WriteTx<'_> {
    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub fn ctx(&self) -> &Context {
        &self.core.ctx
    }

    pub fn default_tz(&self) -> String {
        self.core.default_tz()
    }

    /// Wall-clock time of this transaction. All rows written in one
    /// transaction share it, e.g. stopping one entry and starting the next.
    pub fn now(&self) -> i64 {
        self.now
    }

    /// Fresh stamp; each changed row gets its own HLC.
    pub fn stamp(&self) -> Stamp {
        let hlc = self
            .core
            .hlc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tick(self.now);
        Stamp {
            now: self.now,
            hlc,
            device: self.core.ctx.device_id.clone(),
        }
    }

    pub async fn log_upsert<T: Serialize>(
        &mut self,
        table: &str,
        row_id: &str,
        stamp: &Stamp,
        row: &T,
    ) -> Result<()> {
        let payload = serde_json::to_string(row)?;
        self.log(table, row_id, "upsert", stamp, Some(payload)).await
    }

    pub async fn log_delete(&mut self, table: &str, row_id: &str, stamp: &Stamp) -> Result<()> {
        self.log(table, row_id, "delete", stamp, None).await
    }

    async fn log(
        &mut self,
        table: &str,
        row_id: &str,
        op: &str,
        stamp: &Stamp,
        payload: Option<String>,
    ) -> Result<()> {
        let payload = self.with_owner(table, payload);
        sqlx::query(
            "INSERT INTO change_log (table_name, row_id, op, hlc, payload, workspace_id) VALUES (?, ?, ?, ?, ?, ?)",
        )
            .bind(table)
            .bind(row_id)
            .bind(op)
            .bind(&stamp.hlc)
            .bind(payload)
            .bind(&self.core.ctx.workspace_id)
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }

    /// In a shared workspace the server decides who may change a row by its
    /// `userId`, so rows of one person's own data carry it.
    fn with_owner(&self, table: &str, payload: Option<String>) -> Option<String> {
        const PERSONAL: [&str; 4] = ["absence", "balance_adjustment", "work_settings", "work_schedule"];
        let ctx = &self.core.ctx;
        if !ctx.shared || !PERSONAL.contains(&table) {
            return payload;
        }
        let mut value: serde_json::Value = serde_json::from_str(payload.as_deref()?).ok()?;
        let object = value.as_object_mut()?;
        object
            .entry("userId")
            .or_insert_with(|| serde_json::Value::String(ctx.user_id.clone()));
        Some(value.to_string())
    }

    /// Queues a domain event; it is published when the transaction commits.
    pub fn emit(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await?;
        for event in self.events {
            self.core.publish(event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[tokio::test]
    async fn bootstrap_creates_identity_and_logs_it() {
        let core = Core::open_in_memory(Arc::new(ManualClock::new(1_000)))
            .await
            .unwrap();
        assert!(!core.ctx().workspace_id.is_empty());
        assert!(!core.ctx().user_id.is_empty());
        // workspace + user + 5 built-in absence types
        assert_eq!(core.pending_changes().await.unwrap(), 7);
        assert_eq!(core.list_absence_types().await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn bootstrap_is_stable_across_reopen() {
        let dir = std::env::temp_dir().join(format!("zeima-core-test-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zeima.db");
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_000));
        let first = Core::open(&path, clock.clone()).await.unwrap().ctx().clone();
        let second = Core::open(&path, clock).await.unwrap().ctx().clone();
        assert_eq!(first, second);
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_database_and_its_folder_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir()
            .join(format!("zeima-core-perm-{}", new_id()))
            .join("data");
        let path = dir.join("zeima.db");
        let core = Core::open(&path, Arc::new(ManualClock::new(1_000)))
            .await
            .unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);
        drop(core);
        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}
