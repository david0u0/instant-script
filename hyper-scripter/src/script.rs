use crate::config::Config;
use crate::error::{Contextable, Error, FormatCode::ScriptName as ScriptNameCode, Result};
use crate::script_time::ScriptTime;
use crate::script_type::ScriptType;
use crate::tag::Tag;
use chrono::NaiveDateTime;
use fxhash::FxHashSet as HashSet;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::str::FromStr;

pub const ANONYMOUS: &'static str = ".anonymous";

macro_rules! max {
    ($x:expr) => ( $x );
    ($x:expr, $($xs:expr),+) => {
        {
            use std::cmp::max;
            max($x, max!( $($xs),+ ))
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord)]
pub enum ScriptName {
    Anonymous(u32),
    Named(String),
}
impl FromStr for ScriptName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        s.to_owned().into_script_name()
    }
}
impl ScriptName {
    pub fn valid(s: &str) -> Result<Option<u32>> {
        log::debug!("檢查腳本名：{}", s);
        let reg = regex::Regex::new(r"^\.(\w+)$")?;
        let m = reg.captures(s);
        if let Some(m) = m {
            let id_str = m.get(1).unwrap().as_str();
            match id_str.parse::<u32>() {
                Ok(id) => Ok(Some(id)),
                Err(e) => return Err(Error::Format(ScriptNameCode, s.to_owned())).context(e),
            }
        } else {
            // FIXME: 好好想想什麼樣的腳本名可行，並補上單元測試
            if s.starts_with("-")
                || s.starts_with(".")
                || s.find("..").is_some()
                || s.find(" ").is_some()
                || s.len() == 0
            {
                return Err(Error::Format(ScriptNameCode, s.to_owned()))
                    .context("命名腳本格式有誤");
            }
            Ok(None)
        }
    }
    pub fn namespaces(&self) -> Vec<&'_ str> {
        match self {
            ScriptName::Anonymous(_) => vec![],
            ScriptName::Named(s) => {
                let mut v: Vec<_> = s.split("/").collect();
                v.pop();
                v
            }
        }
    }
    pub fn is_anonymous(&self) -> bool {
        match self {
            ScriptName::Anonymous(_) => true,
            _ => false,
        }
    }
    pub fn key(&self) -> Cow<'_, str> {
        match self {
            ScriptName::Anonymous(id) => Cow::Owned(format!(".{}", id)),
            ScriptName::Named(s) => Cow::Borrowed(s),
        }
    }
    pub fn to_file_path(&self, ty: &ScriptType) -> Result<PathBuf> {
        let mut file_name: String;
        let add_ext = |name: &mut String| -> Result<()> {
            if let Some(ext) = &Config::get()?.get_script_conf(ty)?.ext {
                *name = format!("{}.{}", name, ext);
            }
            Ok(())
        };
        match self {
            ScriptName::Anonymous(id) => {
                file_name = id.to_string();
                let dir: PathBuf = ANONYMOUS.into();
                add_ext(&mut file_name)?;
                Ok(dir.join(file_name))
            }
            ScriptName::Named(name) => {
                file_name = name.to_string();
                add_ext(&mut file_name)?;
                Ok(file_name.into())
            }
        }
    }
}
impl PartialOrd for ScriptName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (ScriptName::Named(n1), ScriptName::Named(n2)) => n1.partial_cmp(n2),
            (ScriptName::Anonymous(i1), ScriptName::Anonymous(i2)) => i1.partial_cmp(i2),
            (ScriptName::Named(_), ScriptName::Anonymous(_)) => Some(Ordering::Less),
            (ScriptName::Anonymous(_), ScriptName::Named(_)) => Some(Ordering::Greater),
        }
    }
}
impl std::fmt::Display for ScriptName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key())
    }
}

#[derive(Debug, Clone)]
pub struct ScriptInfo {
    pub read_time: ScriptTime,
    pub created_time: ScriptTime,
    pub write_time: ScriptTime,
    pub exec_time: Option<ScriptTime<(String, String)>>,
    pub miss_time: Option<ScriptTime>,
    pub exec_done_time: Option<ScriptTime<i32>>,
    pub id: i64,
    pub name: ScriptName,
    pub tags: HashSet<Tag>,
    pub ty: ScriptType,
}

impl ScriptInfo {
    pub fn cp(&self, new_name: ScriptName) -> Self {
        let now = ScriptTime::now(());
        ScriptInfo {
            name: new_name,
            read_time: now.clone(),
            write_time: now.clone(),
            created_time: now,
            exec_time: None,
            ..self.clone()
        }
    }
    pub fn last_time(&self) -> NaiveDateTime {
        fn map<T>(time: &Option<ScriptTime<T>>) -> NaiveDateTime {
            match time {
                Some(time) => **time,
                _ => NaiveDateTime::from_timestamp(1, 0),
            }
        }
        max!(
            *self.read_time,
            map(&self.miss_time),
            map(&self.exec_time),
            map(&self.exec_done_time)
        )
    }
    pub fn file_path(&self) -> Result<PathBuf> {
        self.name.to_file_path(&self.ty)
    }
    pub fn read(&mut self) {
        self.read_time = ScriptTime::now(());
    }
    pub fn write(&mut self) {
        let now = ScriptTime::now(());
        self.read_time = now.clone();
        self.write_time = now;
    }
    pub fn exec(&mut self, content: String, args: &[String]) {
        log::trace!("{:?} 執行內容為 {}", self, content);
        let args_ser = serde_json::to_string(args).unwrap();
        self.exec_time = Some(ScriptTime::now((content, args_ser)));
        // NOTE: no readtime, otherwise it will be hard to tell what event was caused by what operation.
    }
    pub fn exec_done(&mut self, code: i32) {
        log::trace!("{:?} 執行結果為 {}", self, code);
        self.exec_done_time = Some(ScriptTime::now(code));
    }
    pub fn builder(
        id: i64,
        name: ScriptName,
        ty: ScriptType,
        tags: impl Iterator<Item = Tag>,
    ) -> ScriptBuilder {
        ScriptBuilder {
            id,
            name,
            ty,
            tags: tags.collect(),
            read_time: None,
            created_time: None,
            exec_time: None,
            miss_time: None,
            write_time: None,
            exec_done_time: None,
        }
    }
}

pub trait IntoScriptName {
    fn into_script_name(self) -> Result<ScriptName>;
}

impl IntoScriptName for String {
    fn into_script_name(self) -> Result<ScriptName> {
        log::debug!("解析腳本名：{}", self);
        if let Some(id) = ScriptName::valid(&self)? {
            Ok(ScriptName::Anonymous(id))
        } else {
            Ok(ScriptName::Named(self))
        }
    }
}
impl IntoScriptName for ScriptName {
    fn into_script_name(self) -> Result<ScriptName> {
        Ok(self)
    }
}

#[derive(Debug)]
pub struct ScriptBuilder {
    pub name: ScriptName,
    read_time: Option<NaiveDateTime>,
    created_time: Option<NaiveDateTime>,
    write_time: Option<NaiveDateTime>,
    miss_time: Option<NaiveDateTime>,
    exec_time: Option<NaiveDateTime>,
    exec_done_time: Option<NaiveDateTime>,
    id: i64,
    tags: HashSet<Tag>,
    ty: ScriptType,
}

impl ScriptBuilder {
    pub fn exec_time(mut self, time: NaiveDateTime) -> Self {
        self.exec_time = Some(time);
        self
    }
    pub fn exec_done_time(mut self, time: NaiveDateTime) -> Self {
        self.exec_done_time = Some(time);
        self
    }
    pub fn miss_time(mut self, time: NaiveDateTime) -> Self {
        self.miss_time = Some(time);
        self
    }
    pub fn read_time(mut self, time: NaiveDateTime) -> Self {
        self.read_time = Some(time);
        self
    }
    pub fn write_time(mut self, time: NaiveDateTime) -> Self {
        self.write_time = Some(time);
        self
    }
    pub fn created_time(mut self, time: NaiveDateTime) -> Self {
        self.created_time = Some(time);
        self
    }
    pub fn build(self) -> ScriptInfo {
        let now = ScriptTime::now(());
        let created_time = ScriptTime::new_or(self.created_time, now);
        ScriptInfo {
            id: self.id,
            name: self.name,
            ty: self.ty,
            tags: self.tags,
            write_time: ScriptTime::new_or(self.write_time, created_time.clone()),
            read_time: ScriptTime::new_or(self.read_time, created_time.clone()),
            created_time,
            exec_time: self.exec_time.map(|t| ScriptTime::new(t)),
            miss_time: self.miss_time.map(|t| ScriptTime::new(t)),
            exec_done_time: self.exec_done_time.map(|t| ScriptTime::new(t)),
        }
    }
}
