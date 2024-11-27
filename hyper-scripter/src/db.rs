use crate::error::Result;
use crate::util::read_file;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
    SqlitePool,
};
use std::path::{Path, PathBuf};

pub fn get_file() -> PathBuf {
    crate::path::get_home().join(".script_info.db")
}

fn get_sql_file() -> PathBuf {
    crate::path::get_home().join(".script_info.sql")
}

/// 有可能改變 need_journal 的值。
/// 若為初始化，或資料庫已被 journal 鎖住，則不論如何都使用 journal
pub async fn get_pool(need_journal: &mut bool) -> Result<(SqlitePool, bool)> {
    let file = get_file();
    if !file.exists() {
        *need_journal = true;
        let pool = do_migrate_may_force_pre_sql(file, true).await?;
        return Ok((pool, true)); // FIXME: 若有 .script_info.sql 不應視為初始
    }

    let mut opt = SqliteConnectOptions::new().filename(&file);
    if !*need_journal {
        opt = opt.journal_mode(SqliteJournalMode::Off);
    }
    let res = SqlitePool::connect_with(opt).await;
    let pool = match res {
        Err(err) => {
            // 通常是有其它程序用 journal mode 鎖住資料庫，例如正在編輯另一個腳本
            log::warn!("資料庫錯誤 {}，嘗試用 journal 再開一次", err);
            *need_journal = true;
            let opt = SqliteConnectOptions::new().filename(&file);
            SqlitePool::connect_with(opt).await?
        }
        Ok(pool) => pool,
    };
    Ok((pool, false))
}

async fn do_migrate_may_force_pre_sql(
    file: impl AsRef<Path>,
    force_pre_sql: bool,
) -> Result<SqlitePool> {
    let pre_sql = if !force_pre_sql && file.as_ref().exists() {
        None
    } else {
        let sql_file = get_sql_file();
        if sql_file.exists() {
            Some(read_file(&sql_file)?)
        } else {
            None
        }
    };
    let pool = crate::migration::do_migrate_with_pre_sql(file, pre_sql.as_deref()).await?;
    Ok(pool)
}

pub async fn do_migrate(file: impl AsRef<Path>) -> Result<SqlitePool> {
    do_migrate_may_force_pre_sql(file, false).await
}
