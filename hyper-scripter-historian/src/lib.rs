#[macro_use]
extern crate derive_more;

use chrono::NaiveDateTime;
use sqlx::{error::Error as DBError, Pool, Sqlite, SqlitePool};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

mod db;
mod event;
pub mod migration;
pub use event::*;

const ZERO: i64 = 0;

#[derive(Debug, Clone)]
pub struct Historian {
    pool: Arc<RwLock<SqlitePool>>,
    path: PathBuf,
}

async fn raw_record_event(pool: &Pool<Sqlite>, event: DBEvent<'_>) -> Result<i64, DBError> {
    sqlx::query!(
        "
        INSERT INTO events
        (script_id, type, cmd, args, content, time, main_event_id)
        VALUES(?, ?, ?, ?, ?, ?, ?)
        ",
        event.script_id,
        event.ty,
        event.cmd,
        event.args,
        event.content,
        event.time,
        event.main_event_id
    )
    .execute(pool)
    .await?;
    let res = sqlx::query!("SELECT last_insert_rowid() AS id")
        .fetch_one(pool)
        .await?;
    Ok(res.id as i64)
}

#[derive(Clone, Copy)]
struct DBEvent<'a> {
    script_id: i64,
    ty: &'a str,
    cmd: &'a str,
    time: NaiveDateTime,
    args: Option<&'a str>,
    content: Option<&'a str>,
    main_event_id: i64,
}
impl<'a> DBEvent<'a> {
    fn new(script_id: i64, time: NaiveDateTime, ty: &'a str, cmd: &'a str) -> Self {
        DBEvent {
            script_id,
            time,
            ty,
            cmd,
            main_event_id: ZERO,
            content: None,
            args: None,
        }
    }
    fn args(mut self, value: &'a str) -> Self {
        self.args = Some(value);
        self
    }
    fn content(mut self, value: &'a str) -> Self {
        self.content = Some(value);
        self
    }
    fn main_event_id(mut self, value: i64) -> Self {
        self.main_event_id = value;
        self
    }
}

struct Args {
    args: Option<String>,
}
struct Time {
    time: NaiveDateTime,
}
macro_rules! select_last_arg {
    ($ty:ident, $select:literal, $script_id:expr, $offset:expr, $limit:expr) => {{
        static EXEC_TY: &'static str = EventType::Exec.get_str();
        sqlx::query_as_unchecked!(
            $ty,
            "
            WITH args AS (
                SELECT args, max(time) as time FROM events
                WHERE type = ? AND script_id = ? AND NOT ignored
                GROUP BY args
                ORDER BY time DESC LIMIT ? OFFSET ?
            ) SELECT "
                + $select
                + " FROM args
            ",
            EXEC_TY,
            $script_id,
            $limit,
            $offset
        )
    }};
}

macro_rules! ignore_arg {
    ($pool:expr, $cond:literal, $($var:expr),+) => {
        let exec_ty = EventType::Exec.get_str();
        let done_ty = EventType::ExecDone.get_str();
        sqlx::query!(
            "
            UPDATE events SET ignored = true
            WHERE type = ? AND
            "
                + $cond,
            exec_ty,
            $($var),*
        )
        .execute(&*$pool)
        .await?;

        sqlx::query!(
            "
            UPDATE events SET ignored = true
            WHERE type = ? AND main_event_id IN (
                SELECT id FROM events WHERE type = ? AND "
                + $cond
                + "
            )
            ",
            done_ty,
            exec_ty,
            $($var),*
        )
        .execute(&*$pool)
        .await?;
    };
}

#[derive(Debug)]
pub struct IgnoreResult {
    pub script_id: i64,
    pub exec_time: Option<NaiveDateTime>,
    pub exec_done_time: Option<NaiveDateTime>,
}

impl Historian {
    async fn raw_record(&self, event: DBEvent<'_>) -> Result<i64, DBError> {
        let pool = &mut *self.pool.write().unwrap();
        let res = raw_record_event(pool, event).await;
        if res.is_err() {
            log::warn!("資料庫錯誤 {:?}，再試最後一次！", res);
            *pool = db::get_pool(&self.path).await?;
            return raw_record_event(pool, event).await;
        }

        res
    }
    pub async fn new(path: PathBuf) -> Result<Self, DBError> {
        db::get_pool(&path).await.map(|pool| Historian {
            pool: Arc::new(RwLock::new(pool)),
            path,
        })
    }

    pub async fn remove(&self, script_id: i64) -> Result<(), DBError> {
        let pool = self.pool.read().unwrap();
        sqlx::query!("DELETE FROM events WHERE script_id = ?", script_id,)
            .execute(&*pool)
            .await?;
        Ok(())
    }

    pub async fn record(&self, event: &Event<'_>) -> Result<i64, DBError> {
        log::debug!("記錄事件 {:?}", event);
        let ty = event.data.get_type().get_str();
        let time = event.time;
        let cmd = std::env::args().collect::<Vec<_>>().join(" ");
        let mut db_event = DBEvent::new(event.script_id, time, &ty, &cmd);
        let id = match &event.data {
            EventData::Write | EventData::Read => self.raw_record(db_event).await?,
            EventData::Exec { content, args } => {
                let mut content = Some(*content);
                let last_event = sqlx::query!(
                    "
                    SELECT content FROM events
                    WHERE type = ? AND script_id = ? AND NOT content IS NULL
                    ORDER BY time DESC LIMIT 1
                    ",
                    ty,
                    event.script_id
                )
                .fetch_optional(&*self.pool.read().unwrap())
                .await?;
                if let Some(last_event) = last_event {
                    if last_event.content.as_deref() == content {
                        log::debug!("上次執行內容相同，不重複記錄");
                        content = None;
                    }
                }
                db_event.content = content;
                self.raw_record(db_event.args(args)).await?
            }
            EventData::ExecDone {
                code,
                main_event_id,
            } => {
                let exec_ty = EventType::Exec.get_str();
                let ignored_res = sqlx::query!(
                    "SELECT ignored FROM events WHERE type = ? AND id = ?",
                    exec_ty,
                    main_event_id
                )
                .fetch_one(&*self.pool.read().unwrap())
                .await?;
                if ignored_res.ignored {
                    return Ok(ZERO);
                }
                let code = code.to_string();
                self.raw_record(db_event.content(&code).main_event_id(*main_event_id))
                    .await?
            }
        };
        Ok(id)
    }

    pub async fn last_args(&self, id: i64) -> Result<Option<String>, DBError> {
        let ty = EventType::Exec.get_str();
        let res = sqlx::query!(
            "
            SELECT args FROM events
            WHERE type = ? AND script_id = ? AND NOT ignored
            ORDER BY time DESC LIMIT 1
            ",
            ty,
            id
        )
        .fetch_optional(&*self.pool.read().unwrap())
        .await?;
        Ok(res.map(|res| res.args.unwrap_or_default()))
    }

    pub async fn last_args_list(
        &self,
        id: i64,
        limit: u32,
        offset: u32,
    ) -> Result<impl ExactSizeIterator<Item = String>, DBError> {
        let limit = limit as i64;
        let offset = offset as i64;
        let res = select_last_arg!(Args, "args", id, offset, limit)
            .fetch_all(&*self.pool.read().unwrap())
            .await?;
        Ok(res.into_iter().map(|res| res.args.unwrap_or_default()))
    }

    async fn make_ignore_result(&self, script_id: i64) -> Result<IgnoreResult, DBError> {
        Ok(IgnoreResult {
            script_id,
            exec_time: self.last_time_of(script_id, EventType::Exec).await?,
            exec_done_time: self.last_time_of(script_id, EventType::ExecDone).await?,
        })
    }
    pub async fn ignore_args_by_id(&self, event_id: i64) -> Result<Option<IgnoreResult>, DBError> {
        if event_id == ZERO {
            log::info!("試圖忽略零事件，什麼都不做");
            return Ok(None);
        }

        let pool = self.pool.read().unwrap();
        let exec_ty = EventType::Exec.get_str();
        let latest_record = sqlx::query!(
            "
            SELECT id, script_id FROM events
            WHERE type= ? AND script_id = (SELECT script_id FROM events WHERE id = ?)
            ORDER BY time DESC LIMIT 1
            ",
            exec_ty,
            event_id,
        )
        .fetch_one(&*pool)
        .await?;
        // TODO: check if this event is exec?

        ignore_arg!(pool, "id = ?", event_id);

        if latest_record.id == event_id {
            log::info!("ignore last args");
            let ret = self.make_ignore_result(latest_record.script_id).await?;
            return Ok(Some(ret));
        }
        Ok(None)
    }
    pub async fn ignore_args_range(
        &self,
        script_id: i64,
        min: NonZeroU64,
        max: Option<NonZeroU64>,
    ) -> Result<Option<IgnoreResult>, DBError> {
        let min = min.get() as i64 - 1;
        let pool = self.pool.read().unwrap();
        let last_time = select_last_arg!(Time, "time", script_id, min, 1)
            .fetch_one(&*pool)
            .await?
            .time;
        let first_time = if let Some(max) = max {
            let max = max.get() as i64 - 1;
            let t = select_last_arg!(Time, "time", script_id, max, 1)
                .fetch_optional(&*pool)
                .await?;
            t.map(|t| t.time)
        } else {
            None
        };
        log::debug!("刪除最晚事件：{}，最早 {:?}", last_time, first_time);

        if let Some(first_time) = first_time {
            ignore_arg!(
                pool,
                "script_id = ? AND time <= ? AND time > ?",
                script_id,
                last_time,
                first_time
            );
        } else {
            ignore_arg!(pool, "script_id = ? AND time <= ?", script_id, last_time);
        }

        if min == 0 {
            log::info!("ignore last args");
            let ret = self.make_ignore_result(script_id).await?;
            return Ok(Some(ret));
        }
        Ok(None)
    }
    pub async fn ignore_args(
        &self,
        script_id: i64,
        number: NonZeroU64,
    ) -> Result<Option<IgnoreResult>, DBError> {
        let number = number.get();
        let offset = number as i64 - 1;
        let pool = self.pool.read().unwrap();

        let args = select_last_arg!(Args, "args", script_id, offset, 1)
            .fetch_one(&*pool)
            .await?
            .args;

        ignore_arg!(pool, "script_id = ? AND args = ?", script_id, args);

        if offset == 0 {
            log::info!("ignore last args");
            let ret = self.make_ignore_result(script_id).await?;
            return Ok(Some(ret));
        }
        Ok(None)
    }

    pub async fn amend_args_by_id(&self, event_id: i64, args: &str) -> Result<(), DBError> {
        if event_id == ZERO {
            log::info!("試圖修改零事件，什麼都不做");
            return Ok(());
        }

        let exec_ty = EventType::Exec.get_str();
        sqlx::query!(
            "
            UPDATE events SET ignored = false, args = ?
            WHERE type = ? AND id = ?
            ",
            args,
            exec_ty,
            event_id
        )
        .execute(&*self.pool.read().unwrap())
        .await?;
        Ok(())
    }

    pub async fn last_time_of(
        &self,
        script_id: i64,
        ty: EventType,
    ) -> Result<Option<NaiveDateTime>, DBError> {
        let ty = ty.get_str();
        let time = sqlx::query!(
            "
            SELECT time FROM events
            WHERE type = ? AND script_id = ? AND NOT ignored
            ORDER BY time DESC LIMIT 1
            ",
            ty,
            script_id
        )
        .fetch_optional(&*self.pool.read().unwrap())
        .await?;
        Ok(time.map(|t| t.time))
    }

    pub async fn tidy(&self, script_id: i64) -> Result<(), DBError> {
        let pool = self.pool.read().unwrap();
        let exec_ty = EventType::Exec.get_str();
        // XXX: 笑死這啥鬼
        sqlx::query!(
            "
            DELETE FROM events
            WHERE script_id = ?
              AND id NOT IN (
                SELECT
                  (
                    SELECT id FROM events
                    WHERE script_id = ?
                      AND args = e.args
                    ORDER BY time DESC
                    LIMIT 1
                  )
                FROM
                  (
                    SELECT distinct args
                    FROM events
                    WHERE script_id = ?
                      AND NOT ignored
                      AND type = ?
                  ) e
              )
            ",
            script_id,
            script_id,
            script_id,
            exec_ty,
        )
        .execute(&*pool)
        .await?;

        Ok(())
    }
}
