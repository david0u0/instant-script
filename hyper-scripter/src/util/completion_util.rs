use super::{init_repo, print_iter};
use crate::args::{AliasRoot, Completion, Root, Subs};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::fuzzy::{fuzz_with_multifuzz_ratio, is_prefix, FuzzResult};
use crate::path;
use crate::script_repo::RepoEntry;
use std::cmp::Reverse;
use structopt::StructOpt;

const SEP: &str = "/";

fn sort(v: &mut Vec<RepoEntry<'_>>) {
    v.sort_by_key(|s| Reverse(s.last_time()));
}

fn extract_conf(args: &[String]) -> Result<(Config, AliasRoot)> {
    match AliasRoot::from_iter_safe(args) {
        Ok(root) => {
            let p = path::compute_home_path_optional(root.root_args.hs_home.as_ref())?;
            Ok((Config::load(&p)?, root))
        }
        Err(e) => {
            log::warn!("展開別名時出錯 {}", e);
            // NOTE: -V 或 --help 也會走到這裡
            Err(Error::Completion)
        }
    }
}

async fn fuzz_arr<'a>(
    name: &str,
    iter: impl Iterator<Item = RepoEntry<'a>>,
) -> Result<Vec<RepoEntry<'a>>> {
    // TODO: 測試這個複雜的函式，包括前綴和次級結果
    let res = fuzz_with_multifuzz_ratio(name, iter, SEP, 0.6).await?;
    Ok(match res {
        None => vec![],
        Some(FuzzResult::High(t) | FuzzResult::Low(t)) => vec![t],
        Some(FuzzResult::Multi {
            ans,
            others,
            mut still_others,
        }) => {
            let prefix = ans.name.key();
            let mut first_others = vec![];
            let mut prefixed_others = vec![];
            for candidate in others.into_iter() {
                if is_prefix(&*prefix, &*candidate.name.key(), SEP) {
                    prefixed_others.push(candidate);
                } else {
                    first_others.push(candidate);
                }
            }
            first_others.push(ans);

            sort(&mut first_others);
            sort(&mut prefixed_others);
            sort(&mut still_others);
            first_others.append(&mut prefixed_others);
            first_others.append(&mut still_others);
            first_others
        }
    })
}

pub async fn handle_completion(comp: Completion) -> Result {
    match comp {
        Completion::LS { name, args } => {
            let mut new_root = match Root::from_iter_safe(args) {
                Ok(Root {
                    subcmd: Some(Subs::Tags(_)),
                    ..
                }) => {
                    // TODO: 在補全腳本中處理，而不要在這邊
                    return Err(Error::Completion);
                }
                Ok(t) => t,
                Err(e) => {
                    log::warn!("補全時出錯 {}", e);
                    // NOTE: -V 或 --help 也會走到這裡
                    return Err(Error::Completion);
                }
            };
            log::info!("補完模式，參數為 {:?}", new_root);
            new_root.set_home_unless_set()?;
            new_root.sanitize_flags();
            let mut repo = init_repo(new_root.root_args, false).await?;

            let iter = repo.iter_mut(false);
            let scripts = if let Some(name) = name {
                fuzz_arr(&name, iter).await?
            } else {
                let mut t: Vec<_> = iter.collect();
                sort(&mut t);
                t
            };

            print_iter(scripts.iter().map(|s| s.name.key()), " ");

            Ok(())
        }
        Completion::Alias { args } => {
            let (conf, alias_root) = extract_conf(&args)?;
            if let Some(new_args) = alias_root.expand_alias(&args, &conf) {
                print_iter(new_args, " ");
            } else {
                print_iter(args.iter(), " ");
            }
            Ok(())
        }
        Completion::Types { args } => {
            let (conf, _) = extract_conf(&args)?;
            print_iter(conf.types.keys(), " ");
            Ok(())
        }
    }
}
