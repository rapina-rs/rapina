//! Backend-specific SQL generation and claiming strategies for the jobs queue.
//!
//! Each of [`Postgres`], [`Mysql`], and [`Sqlite`] exposes the same surface:
//! building [`Statement`]s for inserts, retries, failures, successes, and
//! (except MySQL) an atomic `claim_batch` that transitions pending → running.

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, FromQueryResult, Statement,
    TransactionTrait, Value,
};
use uuid::Uuid;

use crate::jobs::JobRequest;
use crate::jobs::RapinaJobs;
use crate::jobs::model::JobRow;
use crate::jobs::worker::ResolvedQueueConfig;

/// `last_error` recorded by the reaper on a reclaimed or failed row.
pub(crate) const LEASE_EXPIRED_ERROR: &str =
    "job lease expired before completion (worker crashed or job_timeout exceeded)";

pub struct Postgres;

impl Postgres {
    pub fn build_insert_stmt(req: JobRequest, trace_id: Option<&str>, id: Uuid) -> Statement {
        let sql = format!(
            "INSERT INTO {} \
             ({}, {}, {}, {}, {}, {}, {}, {}) \
             VALUES ($1::uuid, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            RapinaJobs::table_name(),
            RapinaJobs::id(),
            RapinaJobs::job_type(),
            RapinaJobs::queue(),
            RapinaJobs::payload(),
            RapinaJobs::max_retries(),
            RapinaJobs::trace_id(),
            RapinaJobs::run_at(),
            RapinaJobs::created_at(),
        );
        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [
                Value::String(Some(Box::new(id.to_string()))),
                req.job_type.into(),
                req.queue.into(),
                req.payload.into(),
                req.max_retries.into(),
                trace_id.map(ToOwned::to_owned).into(),
            ],
        )
    }

    pub async fn claim_batch(
        db: &DatabaseConnection,
        config: &ResolvedQueueConfig,
    ) -> Result<Vec<JobRow>, DbErr> {
        let stmt = Self::build_claim_stmt(config);
        let rows = db.query_all(stmt).await?;
        rows.iter()
            .map(|row| JobRow::from_query_result(row, ""))
            .collect()
    }

    fn build_claim_stmt(config: &ResolvedQueueConfig) -> Statement {
        let t = RapinaJobs::table_name();
        let id = RapinaJobs::id();
        let st = RapinaJobs::status();
        let q = RapinaJobs::queue();
        let r = RapinaJobs::run_at();
        let sa = RapinaJobs::started_at();
        let lu = RapinaJobs::locked_until();

        let sql = format!(
            r#"WITH claimed AS (
                   SELECT {id} FROM {t}
                   WHERE  {st}  = 'pending'
                     AND  {q}   = $1
                     AND  {r} <= CURRENT_TIMESTAMP
                   ORDER  BY {r} ASC
                   LIMIT  $2
                   FOR UPDATE SKIP LOCKED
               )
               UPDATE {t}
               SET {st}   = 'running',
                   {sa}   = CURRENT_TIMESTAMP,
                   {lu} = CURRENT_TIMESTAMP + make_interval(secs => $3)
               FROM claimed
               WHERE {t}.{id} = claimed.{id}
               RETURNING {t}.*"#
        );

        let values = [
            Value::String(Some(Box::new(config.queue.clone()))),
            Value::Int(Some(config.batch_size)),
            Value::Double(Some(config.job_timeout.as_secs_f64())),
        ];

        Statement::from_sql_and_values(DbBackend::Postgres, &sql, values)
    }

    pub async fn reap_expired(db: &DatabaseConnection, queue: &str) -> Result<(u64, u64), DbErr> {
        let fail = db.execute(Self::build_fail_expired_stmt(queue)).await?;
        let reclaim = db.execute(Self::build_reclaim_stmt(queue)).await?;
        Ok((fail.rows_affected(), reclaim.rows_affected()))
    }

    fn build_fail_expired_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = $1,
                   {st} = 'failed',
                   {fa} = CURRENT_TIMESTAMP
               WHERE {st} = 'running'
                 AND {lu} <= CURRENT_TIMESTAMP
                 AND {q} = $2
                 AND {att} + 1 >= {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    fn build_reclaim_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = $1,
                   {st} = 'pending',
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {st} = 'running'
                 AND {lu} <= CURRENT_TIMESTAMP
                 AND {q} = $2
                 AND {att} + 1 < {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    pub fn build_retry_stmt(job_id: Uuid, error: &str, delay_secs: f64) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let r = RapinaJobs::run_at();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = $1,
                   {st} = 'pending',
                   {r}  = CURRENT_TIMESTAMP + make_interval(secs => $2),
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {id} = $3::uuid AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::Double(Some(delay_secs)),
                Value::String(Some(Box::new(job_id.to_string()))),
            ],
        )
    }

    pub fn build_fail_stmt(job_id: Uuid, error: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = $1,
                   {st} = 'failed',
                   {fa} = CURRENT_TIMESTAMP
               WHERE {id} = $2::uuid AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::String(Some(Box::new(job_id.to_string()))),
            ],
        )
    }

    pub fn build_success_stmt(job_id: Uuid) -> Statement {
        let t = RapinaJobs::table_name();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {st} = 'completed',
                   {fa} = CURRENT_TIMESTAMP,
                   {lu} = NULL
               WHERE {id} = $1::uuid AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            [Value::String(Some(Box::new(job_id.to_string())))],
        )
    }
}

pub struct Mysql;

impl Mysql {
    pub fn build_insert_stmt(req: JobRequest, trace_id: Option<&str>, id: Uuid) -> Statement {
        let sql = format!(
            "INSERT INTO {} \
             ({}, {}, {}, {}, {}, {}, {}, {}) \
             VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            RapinaJobs::table_name(),
            RapinaJobs::id(),
            RapinaJobs::job_type(),
            RapinaJobs::queue(),
            RapinaJobs::payload(),
            RapinaJobs::max_retries(),
            RapinaJobs::trace_id(),
            RapinaJobs::run_at(),
            RapinaJobs::created_at(),
        );
        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [
                Value::Uuid(Some(Box::new(id))),
                req.job_type.into(),
                req.queue.into(),
                req.payload.into(),
                req.max_retries.into(),
                trace_id.map(ToOwned::to_owned).into(),
            ],
        )
    }

    pub async fn claim_batch(
        db: &DatabaseConnection,
        config: &ResolvedQueueConfig,
    ) -> Result<Vec<JobRow>, DbErr> {
        let txn = db.begin().await?;

        let select_sql = Self::build_claim_select(config);
        let rows = txn.query_all(select_sql).await?;
        let ids: Vec<Uuid> = rows
            .iter()
            .map(|r| {
                r.try_get::<Uuid>("", RapinaJobs::id()).or_else(|_| {
                    let s: String = r.try_get("", RapinaJobs::id())?;
                    Uuid::parse_str(&s).map_err(|e| DbErr::Type(format!("invalid uuid: {e}")))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if ids.is_empty() {
            txn.commit().await?;
            return Ok(vec![]);
        }

        let update_sql = Self::build_claim_update(config, &ids);
        txn.execute(update_sql).await?;

        let fetch_sql = Self::build_fetch(&ids);
        let updated = txn.query_all(fetch_sql).await?;
        txn.commit().await?;

        updated
            .iter()
            .map(|row| JobRow::from_query_result(row, ""))
            .collect()
    }

    fn build_claim_select(config: &ResolvedQueueConfig) -> Statement {
        let t = RapinaJobs::table_name();
        let st = RapinaJobs::status();
        let q = RapinaJobs::queue();
        let r = RapinaJobs::run_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"SELECT {id} FROM {t}
               WHERE  {st}  = 'pending'
                 AND  {q}   = ?
                 AND  {r} <= CURRENT_TIMESTAMP
               ORDER  BY {r} ASC
               LIMIT  ?
               FOR UPDATE SKIP LOCKED"#
        );

        let values = [
            Value::String(Some(Box::new(config.queue.clone()))),
            Value::Int(Some(config.batch_size)),
        ];

        Statement::from_sql_and_values(DbBackend::MySql, &sql, values)
    }

    fn build_claim_update(config: &ResolvedQueueConfig, ids: &[Uuid]) -> Statement {
        let id_placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

        let t = RapinaJobs::table_name();
        let st = RapinaJobs::status();
        let sa = RapinaJobs::started_at();
        let lu = RapinaJobs::locked_until();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {st}   = 'running',
                   {sa}   = CURRENT_TIMESTAMP,
                   {lu} = CURRENT_TIMESTAMP + INTERVAL ? MICROSECOND
               WHERE {id} IN ({id_placeholders})"#
        );

        let mut values: Vec<Value> = vec![Value::BigInt(Some(
            (config.job_timeout.as_secs_f64() * 1_000_000.0) as i64,
        ))];
        values.extend(ids.iter().map(|id| Value::Uuid(Some(Box::new(*id)))));

        Statement::from_sql_and_values(DbBackend::MySql, &sql, values)
    }

    fn build_fetch(ids: &[Uuid]) -> Statement {
        let id_placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

        let t = RapinaJobs::table_name();
        let id = RapinaJobs::id();

        let sql = format!(r#"SELECT * FROM {t} WHERE {id} IN ({id_placeholders})"#);

        let values: Vec<Value> = ids
            .iter()
            .map(|id| Value::Uuid(Some(Box::new(*id))))
            .collect();

        Statement::from_sql_and_values(DbBackend::MySql, &sql, values)
    }

    pub async fn reap_expired(db: &DatabaseConnection, queue: &str) -> Result<(u64, u64), DbErr> {
        let fail = db.execute(Self::build_fail_expired_stmt(queue)).await?;
        let reclaim = db.execute(Self::build_reclaim_stmt(queue)).await?;
        Ok((fail.rows_affected(), reclaim.rows_affected()))
    }

    fn build_fail_expired_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'failed',
                   {fa} = CURRENT_TIMESTAMP
               WHERE {st} = 'running'
                 AND {lu} <= CURRENT_TIMESTAMP
                 AND {q} = ?
                 AND {att} + 1 >= {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    fn build_reclaim_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'pending',
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {st} = 'running'
                 AND {lu} <= CURRENT_TIMESTAMP
                 AND {q} = ?
                 AND {att} + 1 < {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    pub fn build_retry_stmt(job_id: Uuid, error: &str, delay_secs: f64) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let r = RapinaJobs::run_at();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'pending',
                   {r}  = CURRENT_TIMESTAMP + INTERVAL ? MICROSECOND,
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::BigInt(Some((delay_secs * 1_000_000.0) as i64)),
                Value::Uuid(Some(Box::new(job_id))),
            ],
        )
    }

    pub fn build_fail_stmt(job_id: Uuid, error: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'failed',
                   {fa} = CURRENT_TIMESTAMP
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::Uuid(Some(Box::new(job_id))),
            ],
        )
    }

    pub fn build_success_stmt(job_id: Uuid) -> Statement {
        let t = RapinaJobs::table_name();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {st} = 'completed',
                   {fa} = CURRENT_TIMESTAMP,
                   {lu} = NULL
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::MySql,
            &sql,
            [Value::Uuid(Some(Box::new(job_id)))],
        )
    }
}

pub struct Sqlite;

impl Sqlite {
    pub fn build_insert_stmt(req: JobRequest, trace_id: Option<&str>, id: Uuid) -> Statement {
        let sql = format!(
            "INSERT INTO {} \
             ({}, {}, {}, {}, {}, {}, {}, {}) \
             VALUES (?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
            RapinaJobs::table_name(),
            RapinaJobs::id(),
            RapinaJobs::job_type(),
            RapinaJobs::queue(),
            RapinaJobs::payload(),
            RapinaJobs::max_retries(),
            RapinaJobs::trace_id(),
            RapinaJobs::run_at(),
            RapinaJobs::created_at(),
        );
        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                Value::String(Some(Box::new(id.to_string()))),
                req.job_type.into(),
                req.queue.into(),
                req.payload.into(),
                req.max_retries.into(),
                trace_id.map(ToOwned::to_owned).into(),
            ],
        )
    }

    pub async fn claim_batch(
        db: &DatabaseConnection,
        config: &ResolvedQueueConfig,
    ) -> Result<Vec<JobRow>, DbErr> {
        let stmt = Self::build_claim_stmt(config);
        let rows = db.query_all(stmt).await?;
        rows.iter()
            .map(|row| JobRow::from_query_result(row, ""))
            .collect()
    }

    fn build_claim_stmt(config: &ResolvedQueueConfig) -> Statement {
        let t = RapinaJobs::table_name();
        let id = RapinaJobs::id();
        let st = RapinaJobs::status();
        let sa = RapinaJobs::started_at();
        let lu = RapinaJobs::locked_until();
        let q = RapinaJobs::queue();
        let r = RapinaJobs::run_at();

        let sql = format!(
            r#"UPDATE {t}
               SET {st}   = 'running',
                   {sa}   = datetime('now'),
                   {lu} = datetime('now', '+' || ? || ' seconds')
               WHERE {id} IN (
                   SELECT {id} FROM {t}
                   WHERE {st}  = 'pending'
                     AND {q}   = ?
                     AND {r} <= datetime('now')
                   ORDER  BY {r} ASC
                   LIMIT  ?
               )
               RETURNING *"#,
        );

        let values = [
            Value::Double(Some(config.job_timeout.as_secs_f64())),
            Value::String(Some(Box::new(config.queue.clone()))),
            Value::Int(Some(config.batch_size)),
        ];

        Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values)
    }

    pub async fn reap_expired(db: &DatabaseConnection, queue: &str) -> Result<(u64, u64), DbErr> {
        let fail = db.execute(Self::build_fail_expired_stmt(queue)).await?;
        let reclaim = db.execute(Self::build_reclaim_stmt(queue)).await?;
        Ok((fail.rows_affected(), reclaim.rows_affected()))
    }

    // datetime('now') writes fixed-width TEXT timestamps, so `<=` compares
    // chronologically; the claim statements rely on the same property.
    fn build_fail_expired_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'failed',
                   {fa} = datetime('now')
               WHERE {st} = 'running'
                 AND {lu} <= datetime('now')
                 AND {q} = ?
                 AND {att} + 1 >= {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    fn build_reclaim_stmt(queue: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let mr = RapinaJobs::max_retries();
        let q = RapinaJobs::queue();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'pending',
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {st} = 'running'
                 AND {lu} <= datetime('now')
                 AND {q} = ?
                 AND {att} + 1 < {mr}"#
        );

        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                Value::String(Some(Box::new(LEASE_EXPIRED_ERROR.to_owned()))),
                Value::String(Some(Box::new(queue.to_owned()))),
            ],
        )
    }

    pub fn build_retry_stmt(job_id: Uuid, error: &str, delay_secs: f64) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let r = RapinaJobs::run_at();
        let lu = RapinaJobs::locked_until();
        let sa = RapinaJobs::started_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'pending',
                   {r}  = datetime('now', '+' || ? || ' seconds'),
                   {lu} = NULL,
                   {sa} = NULL
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::Double(Some(delay_secs)),
                Value::String(Some(Box::new(job_id.to_string()))),
            ],
        )
    }

    pub fn build_fail_stmt(job_id: Uuid, error: &str) -> Statement {
        let t = RapinaJobs::table_name();
        let att = RapinaJobs::attempts();
        let le = RapinaJobs::last_error();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {att} = {att} + 1,
                   {le} = ?,
                   {st} = 'failed',
                   {fa} = datetime('now')
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                Value::String(Some(Box::new(error.to_owned()))),
                Value::String(Some(Box::new(job_id.to_string()))),
            ],
        )
    }

    pub fn build_success_stmt(job_id: Uuid) -> Statement {
        let t = RapinaJobs::table_name();
        let st = RapinaJobs::status();
        let fa = RapinaJobs::finished_at();
        let lu = RapinaJobs::locked_until();
        let id = RapinaJobs::id();

        let sql = format!(
            r#"UPDATE {t}
               SET {st} = 'completed',
                   {fa} = datetime('now'),
                   {lu} = NULL
               WHERE {id} = ? AND {st} = 'running'"#
        );

        Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [Value::String(Some(Box::new(job_id.to_string())))],
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn resolved(queue: &str, batch_size: i32) -> ResolvedQueueConfig {
        ResolvedQueueConfig {
            queue: queue.to_string(),
            poll_interval: Duration::from_secs(5),
            batch_size,
            job_timeout: Duration::from_secs(30),
        }
    }

    // ── claim statements are scoped to one queue ─────────────────────────────

    #[test]
    fn postgres_claim_stmt_scopes_to_a_single_queue() {
        let stmt = Postgres::build_claim_stmt(&resolved("browser", 7));
        assert!(stmt.sql.contains("queue   = $1"));
        assert!(
            !stmt.sql.contains("IN ("),
            "must not reintroduce multi-queue claiming"
        );

        let values = &stmt.values.as_ref().unwrap().0;
        assert_eq!(values.len(), 3, "queue, batch size, lease timeout");
        assert!(matches!(&values[0], Value::String(Some(s)) if s.as_str() == "browser"));
        assert!(matches!(values[1], Value::Int(Some(7))));
        assert!(matches!(values[2], Value::Double(_)));
    }

    #[test]
    fn mysql_claim_select_scopes_to_a_single_queue() {
        let stmt = Mysql::build_claim_select(&resolved("browser", 7));
        assert!(stmt.sql.contains("queue   = ?"));
        assert!(
            !stmt.sql.contains("IN ("),
            "must not reintroduce multi-queue claiming"
        );

        let values = &stmt.values.as_ref().unwrap().0;
        assert_eq!(values.len(), 2, "queue, batch size");
        assert!(matches!(&values[0], Value::String(Some(s)) if s.as_str() == "browser"));
        assert!(matches!(values[1], Value::Int(Some(7))));
    }

    #[test]
    fn sqlite_claim_stmt_scopes_to_a_single_queue() {
        let stmt = Sqlite::build_claim_stmt(&resolved("browser", 7));
        assert!(stmt.sql.contains("queue   = ?"));

        // `IN (` legitimately appears here as `WHERE id IN (SELECT ...)`, so the
        // anti-regression check is on the bindings. Their order is not the same
        // as the other two backends: the lease timeout sits in the SET clause,
        // above the queue in the subquery, above the LIMIT.
        let values = &stmt.values.as_ref().unwrap().0;
        assert_eq!(values.len(), 3, "lease timeout, queue, batch size");
        assert!(
            matches!(values[0], Value::Double(_)),
            "job_timeout binds first, from the SET clause"
        );
        assert!(matches!(&values[1], Value::String(Some(s)) if s.as_str() == "browser"));
        assert!(matches!(values[2], Value::Int(Some(7))));
    }

    // ── reaper statements are scoped to one queue ────────────────────────────

    #[test]
    fn postgres_fail_expired_stmt_fails_at_error_path_boundary() {
        let stmt = Postgres::build_fail_expired_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= CURRENT_TIMESTAMP"));
        assert!(stmt.sql.contains("attempts + 1 >= max_retries"));
        assert!(stmt.sql.contains("last_error = $1"));
        assert!(stmt.sql.contains("queue = $2"));
    }

    #[test]
    fn postgres_reclaim_stmt_uses_opposite_boundary() {
        let stmt = Postgres::build_reclaim_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= CURRENT_TIMESTAMP"));
        assert!(stmt.sql.contains("attempts + 1 < max_retries"));
        assert!(stmt.sql.contains("locked_until = NULL"));
        assert!(stmt.sql.contains("queue = $2"));
    }

    #[test]
    fn mysql_fail_expired_stmt_fails_at_error_path_boundary() {
        let stmt = Mysql::build_fail_expired_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= CURRENT_TIMESTAMP"));
        assert!(stmt.sql.contains("attempts + 1 >= max_retries"));
        assert!(stmt.sql.contains("last_error = ?"));
        assert!(stmt.sql.contains("queue = ?"));
    }

    #[test]
    fn mysql_reclaim_stmt_uses_opposite_boundary() {
        let stmt = Mysql::build_reclaim_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= CURRENT_TIMESTAMP"));
        assert!(stmt.sql.contains("attempts + 1 < max_retries"));
        assert!(stmt.sql.contains("locked_until = NULL"));
        assert!(stmt.sql.contains("queue = ?"));
    }

    #[test]
    fn sqlite_fail_expired_stmt_fails_at_error_path_boundary() {
        let stmt = Sqlite::build_fail_expired_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= datetime('now')"));
        assert!(stmt.sql.contains("attempts + 1 >= max_retries"));
        assert!(stmt.sql.contains("last_error = ?"));
        assert!(stmt.sql.contains("queue = ?"));
    }

    #[test]
    fn sqlite_reclaim_stmt_uses_opposite_boundary() {
        let stmt = Sqlite::build_reclaim_stmt("browser");
        assert!(stmt.sql.contains("status = 'running'"));
        assert!(stmt.sql.contains("locked_until <= datetime('now')"));
        assert!(stmt.sql.contains("attempts + 1 < max_retries"));
        assert!(stmt.sql.contains("locked_until = NULL"));
        assert!(stmt.sql.contains("queue = ?"));
    }
}
