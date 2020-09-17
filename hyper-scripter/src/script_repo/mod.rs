use crate::error::Result;
use crate::historian::{self, Event, EventData, EventType};
use crate::script::{AsScriptName, ScriptInfo, ScriptName};
use crate::tag::{Tag, TagFilterGroup};
use async_trait::async_trait;
use sqlx::SqlitePool;
use std::collections::HashMap;

pub mod helper;
use helper::*;

pub type ScriptRepoEntry<'a, 'b> = RepoEntry<'a, 'b, SqlitePool>;

#[async_trait]
impl Environment for SqlitePool {
    async fn handle_change<'a>(&self, info: &ScriptInfo<'a>) -> Result {
        log::debug!("開始修改資料庫 {:?}", info);
        let name_cow = info.name.key();
        let name = name_cow.as_ref();
        let tags = join_tags(&info.tags);
        let category = info.ty.as_ref();
        let write_time = *info.write_time;
        sqlx::query!(
            "UPDATE script_infos SET name = ?, tags = ?, category = ?, write_time = ? where id = ?",
            name,
            tags,
            category,
            write_time,
            info.id,
        )
        .execute(self)
        .await?;

        if info.read_time.has_changed() {
            log::debug!("{:?} 的讀取事件", info.name);
            historian::record(
                Event {
                    script_id: info.id,
                    data: EventData::Read,
                },
                &self,
            )
            .await?;
        }
        if info.exec_time.map_or(false, |t| t.has_changed()) {
            log::debug!("{:?} 的執行事件", info.name);
            historian::record(
                Event {
                    script_id: info.id,
                    data: EventData::Exec("content".to_owned()), // FIXME: !!
                },
                &self,
            )
            .await?;
        }

        Ok(())
    }
}

fn join_tags(tags: &[Tag]) -> String {
    let tags_arr: Vec<&str> = tags.iter().map(|t| t.as_ref()).collect();
    tags_arr.join(",")
}

#[derive(Debug)]
pub struct ScriptRepo<'a> {
    map: HashMap<String, ScriptInfo<'a>>,
    hidden_map: HashMap<String, ScriptInfo<'a>>,
    latest_name: Option<String>,
    pool: SqlitePool,
}

impl<'a> ScriptRepo<'a> {
    pub fn iter(&self) -> impl Iterator<Item = &ScriptInfo> {
        self.map.iter().map(|(_, info)| info)
    }
    pub fn iter_mut<'b>(&'b mut self) -> Iter<'a, 'b, SqlitePool> {
        Iter {
            iter: self.map.iter_mut(),
            env: &self.pool,
        }
    }
    pub async fn new<'b>(pool: SqlitePool) -> Result<ScriptRepo<'b>> {
        let scripts = sqlx::query!("SELECT * from script_infos ORDER BY id")
            .fetch_all(&pool)
            .await?;
        let last_read_records = historian::last_time_of(EventType::Read, &pool).await?;
        let last_exec_records = historian::last_time_of(EventType::Exec, &pool).await?;
        let mut last_read: &[_] = &last_read_records;
        let mut last_exec: &[_] = &last_exec_records;
        let mut map: HashMap<String, ScriptInfo> = Default::default();
        for script in scripts.into_iter() {
            use std::str::FromStr;

            let name = script.name;
            log::trace!("載入腳本：{}, {}, {}", name, script.category, script.tags);
            let script_name = name.as_script_name().unwrap().into_static(); // TODO: 正確實作 from string

            let exec_time = match last_exec.first() {
                Some((id, time)) => {
                    if *id == script.id {
                        last_exec = &last_exec[1..last_exec.len()];
                        Some(*time)
                    } else {
                        None
                    }
                }
                None => None,
            };
            let read_time = match last_read.first() {
                Some((id, time)) => {
                    if *id == script.id {
                        last_read = &last_read[1..last_read.len()];
                        Some(*time)
                    } else {
                        None
                    }
                }
                None => None,
            }; // TODO: 真的可以是空值嗎？

            let script = ScriptInfo::new(
                script.id,
                script_name,
                script.category.into(),
                script.tags.split(",").filter_map(|s| Tag::from_str(s).ok()),
                exec_time,
                script.created_time,
                script.write_time,
                read_time,
            );
            map.insert(name, script);
        }
        Ok(ScriptRepo {
            map,
            pool,
            hidden_map: Default::default(),
            latest_name: None,
        })
    }
    // fn latest_mut_no_cache(&mut self) -> Option<&mut ScriptInfo<'a>> {
    //     let latest = self.map.iter_mut().max_by_key(|(_, info)| info.last_time());
    //     if let Some((name, info)) = latest {
    //         self.latest_name = Some(name.clone());
    //         Some(info)
    //     } else {
    //         None
    //     }
    // }
    pub fn latest_mut(&mut self, n: usize) -> Option<ScriptRepoEntry<'a, '_>> {
        // if let Some(name) = &self.latest_name {
        //     // FIXME: 一旦 rust nll 進化就修掉這段
        //     if self.map.contains_key(name) {
        //         return self.map.get_mut(name);
        //     }
        //     log::warn!("快取住的最新資訊已經不見了…？重找一次");
        // }
        // self.latest_mut_no_cache()
        let mut v: Vec<_> = self.map.iter_mut().map(|(_, s)| s).collect();
        v.sort_by_key(|s| s.last_time());
        if v.len() >= n {
            // SAFETY: 從向量中讀一個可變指針安啦
            let t = unsafe { std::ptr::read(&v[v.len() - n]) };
            Some(RepoEntry {
                info: t,
                env: &self.pool,
            })
        } else {
            None
        }
    }
    pub fn get_mut(&mut self, name: &ScriptName) -> Option<ScriptRepoEntry<'a, '_>> {
        match self.map.get_mut(&*name.key()) {
            None => None,
            Some(info) => Some(RepoEntry {
                info,
                env: &self.pool,
            }),
        }
    }
    pub fn get_hidden_mut(&mut self, name: &ScriptName) -> Option<&mut ScriptInfo<'a>> {
        self.hidden_map.get_mut(&*name.key())
    }
    pub async fn remove<'c>(&mut self, name: &ScriptName<'c>) -> Result {
        if let Some(info) = self.map.remove(&*name.key()) {
            log::debug!("從資料庫刪除腳本 {:?}", info);
            sqlx::query!("DELETE from script_infos where id = ?", info.id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    pub async fn upsert<'b, F: FnOnce() -> ScriptInfo<'a>>(
        &mut self,
        name: &ScriptName<'b>,
        default: F,
    ) -> Result<ScriptRepoEntry<'a, '_>> {
        let entry = self.map.entry(name.key().into_owned());
        use std::collections::hash_map::Entry::*;
        let exist = match &entry {
            Vacant(_) => false,
            _ => true,
        };
        let mut info = self
            .map
            .entry(name.key().into_owned())
            .or_insert_with(default);
        if !exist {
            log::debug!("往資料庫塞新腳本 {:?}", info);
            let name_cow = info.name.key();
            let name = name_cow.as_ref();
            let category = info.ty.as_ref();
            let tags = join_tags(&info.tags);
            sqlx::query!(
                "
                INSERT INTO script_infos (name, category, tags)
                VALUES(?, ?, ?)
                ",
                name,
                category,
                tags,
            )
            .execute(&self.pool)
            .await?;
            log::debug!("往資料庫新增腳本成功");
            let id = sqlx::query!("SELECT last_insert_rowid() as id")
                .fetch_one(&self.pool)
                .await?
                .id;
            log::debug!("得到新腳本 id {}", id);
            info.id = id as i64;
        }
        Ok(RepoEntry {
            info,
            env: &self.pool,
        })
    }
    pub fn filter_by_tag(&mut self, filter: &TagFilterGroup) {
        // TODO: 優化
        log::debug!("根據標籤 {:?} 進行篩選", filter);
        let drain = self.map.drain();
        let mut map = HashMap::new();
        for (key, info) in drain {
            if filter.filter(&info.tags) {
                log::trace!("腳本 {:?} 通過篩選", info.name);
                map.insert(key, info);
            } else {
                log::trace!("掰掰，{:?}", info.name);
                self.hidden_map.insert(key, info);
            }
        }
        self.map = map;
    }
}