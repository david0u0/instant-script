use crate::args::Subs;
use crate::color::Stylize;
use crate::config::Config;
use crate::env_pair::EnvPair;
use crate::error::{Contextable, Error, RedundantOpt, Result};
use crate::extract_msg::extract_env_from_content_help_aware;
use crate::path;
use crate::process_lock::{ProcessLockRead, ProcessLockWrite};
use crate::query::{self, EditQuery, ScriptQuery};
use crate::script::{IntoScriptName, ScriptInfo, ScriptName};
use crate::script_repo::{RepoEntry, ScriptRepo, Visibility};
use crate::script_type::{iter_default_templates, ScriptFullType, ScriptType};
use crate::tag::{Tag, TagSelector};
use fxhash::FxHashSet as HashSet;
use std::fs::{create_dir_all, read_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct EditTagArgs {
    pub content: TagSelector,
    /// 命令行參數裡帶著 tag 選項，例如 hs edit --tag some-tag edit
    pub explicit_tag: bool,
    /// 命令行參數裡帶著 select 選項，例如 hs --select some-tag edit
    pub explicit_select: bool,
}

pub async fn mv(
    entry: &mut RepoEntry<'_>,
    new_name: Option<ScriptName>,
    ty: Option<ScriptType>,
    tags: Option<TagSelector>,
) -> Result {
    if ty.is_some() || new_name.is_some() {
        let og_path = path::open_script(&entry.name, &entry.ty, Some(true))?;
        let new_name = new_name.as_ref().unwrap_or(&entry.name);
        let new_ty = ty.as_ref().unwrap_or(&entry.ty);
        let new_path = path::open_script(new_name, new_ty, None)?; // NOTE: 不判斷存在性，因為接下來要對新舊腳本同路徑的狀況做特殊處理
        if new_path != og_path {
            log::debug!("改動腳本檔案：{:?} -> {:?}", og_path, new_path);
            if new_path.exists() {
                return Err(Error::PathExist(new_path).context("移動成既存腳本"));
            }
            super::mv(&og_path, &new_path)?;
        } else {
            log::debug!("相同的腳本檔案：{:?}，不做檔案處理", og_path);
        }
    }

    entry
        .update(|info| {
            if let Some(ty) = ty {
                info.ty = ty;
            }
            if let Some(name) = new_name {
                info.name = name.clone();
            }
            if let Some(tags) = tags {
                info.append_tags(tags);
            }
            info.write();
        })
        .await?;
    Ok(())
}

fn create<F: FnOnce(String) -> Error>(
    query: ScriptQuery,
    script_repo: &mut ScriptRepo,
    ty: &ScriptType,
    tags: &EditTagArgs,
    f: F,
) -> Result<(ScriptName, PathBuf)> {
    if tags.explicit_select {
        return Err(RedundantOpt::Selector.into());
    }

    let name = query.into_script_name()?;
    log::debug!("打開新命名腳本：{:?}", name);
    if script_repo.get_mut(&name, Visibility::All).is_some() {
        return Err(f(name.to_string()));
    }

    let p =
        path::open_script(&name, ty, None).context(format!("打開新命名腳本失敗：{:?}", name))?;
    if p.exists() {
        if p.is_dir() {
            return Err(Error::PathExist(p).context("與目錄撞路徑"));
        }
        check_path_collision(&p, script_repo)?;
        log::warn!("編輯野生腳本！");
    } else {
        // NOTE: 創建資料夾
        if let Some(parent) = p.parent() {
            super::handle_fs_res(&[&p], create_dir_all(parent))?;
        }
    }
    Ok((name, p))
}

// XXX 到底幹嘛把新增和編輯的邏輯攪在一處呢…？
pub async fn edit_or_create(
    edit_query: EditQuery<ScriptQuery>,
    script_repo: &'_ mut ScriptRepo,
    ty: Option<ScriptFullType>,
    tags: EditTagArgs,
) -> Result<(PathBuf, RepoEntry<'_>, Option<ScriptType>)> {
    let final_ty: ScriptFullType;

    let (script_name, script_path) = if let EditQuery::Query(query) = edit_query {
        if let Some(ty) = ty {
            final_ty = ty;
            create(query, script_repo, &final_ty.ty, &tags, |name| {
                log::error!("與已存在的腳本撞名");
                Error::ScriptExist(name.to_string())
            })?
        } else {
            match query::do_script_query(&query, script_repo, false, false).await {
                Err(Error::DontFuzz) | Ok(None) => {
                    final_ty = Default::default();
                    create(query, script_repo, &final_ty.ty, &tags, |name| {
                        log::error!("與被篩掉的腳本撞名");
                        Error::ScriptIsFiltered(name.to_string())
                    })?
                }
                Ok(Some(entry)) => {
                    if tags.explicit_tag {
                        return Err(RedundantOpt::Tag.into());
                    }
                    log::debug!("打開既有命名腳本：{:?}", entry.name);
                    let p = path::open_script(&entry.name, &entry.ty, Some(true))
                        .context(format!("打開命名腳本失敗：{:?}", entry.name))?;
                    // NOTE: 直接返回
                    // FIXME: 一旦 NLL 進化就修掉這段雙重詢問
                    // return Ok((p, entry));
                    let n = entry.name.clone();
                    return Ok((p, script_repo.get_mut(&n, Visibility::All).unwrap(), None));
                }
                Err(e) => return Err(e),
            }
        }
    } else {
        if tags.explicit_select {
            return Err(RedundantOpt::Selector.into());
        }
        final_ty = ty.unwrap_or_default();
        log::debug!("打開新匿名腳本");
        path::open_new_anonymous(&final_ty.ty).context("打開新匿名腳本失敗")?
    };

    log::info!("編輯 {:?}", script_name);

    let ScriptFullType { ty, sub } = final_ty;
    // 這裡的 or_insert 其實永遠會發生，所以無需用閉包來傳
    let entry = script_repo
        .entry(&script_name)
        .or_insert(
            ScriptInfo::builder(0, script_name, ty, tags.content.into_allowed_iter()).build(),
        )
        .await?;

    Ok((script_path, entry, sub))
}

fn run(
    script_path: &Path,
    info: &ScriptInfo,
    remaining: &[String],
    hs_tmpl_val: &super::TmplVal<'_>,
    remaining_envs: &[EnvPair],
) -> Result<()> {
    let conf = Config::get();
    let ty = &info.ty;

    let script_conf = conf.get_script_conf(ty)?;
    let cmd_str = if let Some(cmd) = &script_conf.cmd {
        cmd
    } else {
        return Err(Error::PermissionDenied(vec![script_path.to_path_buf()]));
    };

    let env = conf.gen_env(hs_tmpl_val, true)?;
    let ty_env = script_conf.gen_env(hs_tmpl_val)?;

    let pre_run_script = prepare_pre_run(None)?;
    let (cmd, shebang) = super::shebang_handle::handle(&pre_run_script)?;
    let args = shebang
        .iter()
        .map(|s| s.as_ref())
        .chain(std::iter::once(pre_run_script.as_os_str()))
        .chain(remaining.iter().map(|s| s.as_ref()));

    let set_cmd_envs = |cmd: &mut Command| {
        cmd.envs(ty_env.iter().map(|(a, b)| (a, b)));
        cmd.envs(env.iter().map(|(a, b)| (a, b)));
        cmd.envs(remaining_envs.iter().map(|p| (&p.key, &p.val)));
    };

    let mut cmd = super::create_cmd(cmd, args);
    set_cmd_envs(&mut cmd);

    let stat = super::run_cmd(cmd)?;
    log::info!("預腳本執行結果：{:?}", stat);
    if !stat.success() {
        // TODO: 根據返回值做不同表現
        let code = stat.code().unwrap_or_default();
        return Err(Error::PreRunError(code));
    }

    let args = script_conf.args(hs_tmpl_val)?;
    let full_args = args
        .iter()
        .map(|s| s.as_str())
        .chain(remaining.iter().map(|s| s.as_str()));

    let mut cmd = super::create_cmd(&cmd_str, full_args);
    set_cmd_envs(&mut cmd);

    let stat = super::run_cmd(cmd)?;
    log::info!("程式執行結果：{:?}", stat);
    if !stat.success() {
        let code = stat.code().unwrap_or_default();
        Err(Error::ScriptError(code))
    } else {
        Ok(())
    }
}
pub async fn run_n_times(
    repeat: u64,
    dummy: bool,
    entry: &mut RepoEntry<'_>,
    mut args: Vec<String>,
    res: &mut Vec<Error>,
    use_previous: bool,
    error_no_previous: bool,
    dir: Option<PathBuf>,
) -> Result {
    log::info!("執行 {:?}", entry.name);
    super::hijack_ctrlc_once();

    let mut env_vec = vec![];
    if use_previous {
        let historian = &entry.get_env().historian;
        match historian.previous_args(entry.id, dir.as_deref()).await? {
            None if error_no_previous => {
                return Err(Error::NoPreviousArgs);
            }
            None => log::warn!("無前一次參數，當作空的"),
            Some((arg_str, envs_str)) => {
                log::debug!("撈到前一次呼叫的參數 {}", arg_str);
                let mut prev_arg_vec: Vec<String> =
                    serde_json::from_str(&arg_str).context(format!("反序列失敗 {}", arg_str))?;
                env_vec =
                    serde_json::from_str(&envs_str).context(format!("反序列失敗 {}", envs_str))?;
                prev_arg_vec.extend(args.into_iter());
                args = prev_arg_vec;
            }
        }
    }

    let here = path::normalize_path(".").ok();
    let script_path = path::open_script(&entry.name, &entry.ty, Some(true))?;
    let content = super::read_file(&script_path)?;

    if !Config::get_no_caution()
        && Config::get().caution_tags.select(&entry.tags, &entry.ty) == Some(true)
    {
        let ty = super::get_display_type(&entry.ty);
        let name = &entry.name;
        let msg = format!(
            "{} requires extra caution. Are you sure?",
            name.stylize().color(ty.color()).bold()
        );
        let yes = super::prompt(msg, false)?;
        if !yes {
            return Err(Error::Caution);
        }
    }

    let mut hs_env_desc = vec![];
    for (need_save, line) in extract_env_from_content_help_aware(&content) {
        hs_env_desc.push(line.to_owned());
        if need_save {
            EnvPair::process_line(line, &mut env_vec);
        }
    }
    EnvPair::sort(&mut env_vec);
    let env_record = serde_json::to_string(&env_vec)?;

    let run_id = entry
        .update(|info| info.exec(content, &args, env_record, here))
        .await?;

    if dummy {
        log::info!("--dummy 不用真的執行，提早退出");
        return Ok(());
    }
    // Start packing hs tmpl val
    // SAFETY: 底下所有對 `entry` 的借用，都不會被更後面的 `entry.update` 影響
    let mut hs_tmpl_val = super::TmplVal::new();
    let hs_name = entry.name.key();
    let hs_name = hs_name.as_ref() as *const str;
    let hs_name = unsafe { &*hs_name };
    let hs_tags = &entry.tags as *const HashSet<Tag>;
    let content = entry.exec_time.as_ref().unwrap().data().unwrap().0.as_str() as *const str;
    hs_tmpl_val.path = Some(&script_path);
    hs_tmpl_val.run_id = Some(run_id);
    hs_tmpl_val.tags = unsafe { &*hs_tags }.iter().map(|t| t.as_ref()).collect();
    hs_tmpl_val.env_desc = hs_env_desc;
    hs_tmpl_val.name = Some(hs_name);
    hs_tmpl_val.content = Some(unsafe { &*content });
    // End packing hs tmpl val

    let mut lock = ProcessLockWrite::new(run_id, entry.id, hs_name, &args)?;
    let guard = lock.try_write_info()?;
    for _ in 0..repeat {
        let run_res = run(&script_path, &*entry, &args, &hs_tmpl_val, &env_vec);
        let ret_code: i32;
        match run_res {
            Err(Error::ScriptError(code)) => {
                ret_code = code;
                res.push(run_res.unwrap_err());
            }
            Err(e) => return Err(e),
            Ok(_) => ret_code = 0,
        }
        entry
            .update(|info| info.exec_done(ret_code, run_id))
            .await?;
    }
    if res.is_empty() {
        ProcessLockWrite::mark_sucess(guard);
    }
    Ok(())
}

pub async fn load_utils(script_repo: &mut ScriptRepo) -> Result {
    for u in hyper_scripter_util::get_all().iter() {
        log::info!("載入小工具 {}", u.name);
        let name = u.name.to_owned().into_script_name()?;
        if script_repo.get_mut(&name, Visibility::All).is_some() {
            log::warn!("已存在的小工具 {:?}，跳過", name);
            continue;
        }
        let ty = u.ty.parse()?;
        let tags: Vec<Tag> = if u.is_hidden {
            vec!["util".parse().unwrap(), "hide".parse().unwrap()]
        } else {
            vec!["util".parse().unwrap()]
        };
        let p = path::open_script(&name, &ty, Some(false))?;

        // NOTE: 創建資料夾
        if let Some(parent) = p.parent() {
            super::handle_fs_res(&[&p], create_dir_all(parent))?;
        }

        let entry = script_repo
            .entry(&name)
            .or_insert(ScriptInfo::builder(0, name, ty, tags.into_iter()).build())
            .await?;
        super::prepare_script(&p, &*entry, None, true, &[u.content])?;
    }
    Ok(())
}

pub fn prepare_pre_run(content: Option<&str>) -> Result<PathBuf> {
    let p = path::get_home().join(path::HS_PRE_RUN);
    if content.is_some() || !p.exists() {
        let content = content.unwrap_or_else(|| include_str!("hs_prerun"));
        log::info!("寫入預執行腳本 {:?} {}", p, content);
        super::write_file(&p, content)?;
    }
    Ok(p)
}

pub fn load_templates() -> Result {
    for (ty, tmpl) in iter_default_templates() {
        let tmpl_path = path::get_template_path(&ty)?;
        if tmpl_path.exists() {
            continue;
        }
        super::write_file(&tmpl_path, tmpl)?;
    }
    Ok(())
}

/// 判斷是否需要寫入主資料庫（script_infos 表格）
pub fn need_write(arg: &Subs) -> bool {
    use Subs::*;
    match arg {
        Edit { .. } => true,
        CP { .. } => true,
        RM { .. } => true,
        LoadUtils { .. } => true,
        MV {
            ty,
            tags,
            new,
            origin: _,
        } => {
            // TODO: 好好測試這個
            ty.is_some() || tags.is_some() || new.is_some()
        }
        _ => false,
    }
}

use super::PrepareRespond;
pub async fn after_script(
    entry: &mut RepoEntry<'_>,
    path: &Path,
    prepare_resp: &Option<PrepareRespond>,
) -> Result {
    let mut record_write = true;
    match prepare_resp {
        None => {
            log::debug!("不執行後處理");
        }
        Some(PrepareRespond { is_new, time }) => {
            let modified = super::file_modify_time(path)?;
            if time >= &modified {
                if *is_new {
                    log::info!("新腳本未變動，應刪除之");
                    return Err(Error::EmptyCreate);
                } else {
                    log::info!("舊腳本未變動，不記錄寫事件（只記讀事件）");
                    record_write = false;
                }
            }
        }
    }
    if record_write {
        entry.update(|info| info.write()).await?;
    }
    Ok(())
}

fn check_path_collision(p: &Path, script_repo: &mut ScriptRepo) -> Result {
    for script in script_repo.iter_mut(Visibility::All) {
        let script_p = path::open_script(&script.name, &script.ty, None)?;
        if &script_p == p {
            return Err(Error::PathExist(script_p).context("與既存腳本撞路徑"));
        }
    }
    Ok(())
}

pub fn get_all_active_process_locks() -> Result<Vec<ProcessLockRead>> {
    let dir_path = path::get_process_lock_dir()?;
    let dir = super::handle_fs_res(&[&dir_path], read_dir(&dir_path))?;
    let mut ret = vec![];
    for entry in dir {
        let file_name = entry?.file_name();

        // TODO: concurrent?
        let file_name = file_name
            .to_str()
            .ok_or_else(|| Error::msg("檔案實體為空...?"))?;

        let file_path = dir_path.join(file_name);
        let mut builder = ProcessLockRead::builder(file_path, file_name)?;

        if builder.get_can_write()? {
            log::info!("remove inactive file lock {:?}", builder.path);
            super::remove(&builder.path)?;
        } else {
            log::info!("found active file lock {:?}", builder.path);
            ret.push(builder.build()?);
        }
    }

    Ok(ret)
}
