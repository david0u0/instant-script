use crate::config::{Alias, Config, PromptLevel, Recent};
use crate::env_pair::EnvPair;
use crate::error::{DisplayError, DisplayResult, Result};
use crate::list::Grouping;
use crate::path;
use crate::query::{EditQuery, ListQuery, RangeQuery, ScriptOrDirQuery, ScriptQuery};
use crate::script_type::{ScriptFullType, ScriptType};
use crate::tag::TagSelector;
use crate::to_display_args;
use crate::Either;
use crate::APP_NAME;
use clap::{CommandFactory, Error as ClapError, Parser};
use serde::Serialize;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;

mod completion;
pub use completion::*;
mod tags;
pub use tags::*;
mod help_str;
mod types;
use help_str::*;
pub use types::*;

#[derive(Parser, Debug, Serialize)]
pub struct RootArgs {
    #[clap(short = 'H', long, help = "Path to hyper script home")]
    pub hs_home: Option<String>,
    #[clap(long, hide = true)]
    pub dump_args: bool,
    #[clap(long, global = true, help = "Don't record history")]
    pub no_trace: bool,
    #[clap(
        long,
        global = true,
        conflicts_with = "no-trace",
        help = "Don't affect script time order (but still record history and affect time filter)"
    )]
    pub humble: bool,
    #[clap(short = 'A', long, global = true, help = "Show scripts NOT within recent days", conflicts_with_all = &["all"])]
    pub archaeology: bool,
    #[clap(long)]
    pub no_alias: bool,
    #[clap(
        short,
        long,
        global = true,
        conflicts_with = "all",
        number_of_values = 1,
        help = "Select by tags, e.g. `all,^remove`"
    )]
    pub select: Vec<TagSelector>,
    #[clap(
        long,
        conflicts_with = "all",
        number_of_values = 1,
        help = "Toggle named selector temporarily"
    )]
    pub toggle: Vec<String>, // TODO: new type?
    #[clap(
        short,
        long,
        global = true,
        conflicts_with = "recent",
        help = "Shorthand for `-s=all,^remove --timeless`"
    )]
    all: bool,
    #[clap(long, global = true, help = "Show scripts within recent days.")]
    pub recent: Option<u32>,
    #[clap(
        long,
        global = true,
        help = "Show scripts of all time.",
        conflicts_with = "recent"
    )]
    pub timeless: bool,
    #[clap(long, possible_values(&["never", "always", "smart", "on-multi-fuzz"]), help = "Prompt level of fuzzy finder.")]
    pub prompt_level: Option<PromptLevel>,
    #[clap(long, help = "Run caution scripts without warning")]
    pub no_caution: bool,
}

#[derive(Parser, Debug, Serialize)]
#[clap(about, author, version)]
#[clap(allow_hyphen_values = true, args_override_self = true)] // NOTE: 我們需要那個 `allow_hyphen_values` 來允許 hs --dummy 這樣的命令
pub struct Root {
    #[clap(skip)]
    #[serde(skip)]
    is_from_alias: bool,
    #[clap(flatten)]
    pub root_args: RootArgs,
    #[clap(subcommand)]
    pub subcmd: Option<Subs>,
}

#[derive(Parser, Debug, Serialize)]
pub enum AliasSubs {
    #[clap(external_subcommand)]
    Other(Vec<String>),
}
#[derive(Parser, Debug, Serialize)]
#[clap(
    args_override_self = true,
    allow_hyphen_values = true,
    disable_help_flag = true,
    disable_help_subcommand = true
)]
pub struct AliasRoot {
    #[clap(flatten)]
    pub root_args: RootArgs,
    #[clap(subcommand)]
    pub subcmd: Option<AliasSubs>,
}
impl AliasRoot {
    fn find_alias<'a>(&'a self, conf: &'a Config) -> Option<(&'a Alias, &'a [String])> {
        match &self.subcmd {
            None => None,
            Some(AliasSubs::Other(v)) => {
                let first = v.first().unwrap().as_str();
                if let Some(alias) = conf.alias.get(first) {
                    log::info!("別名 {} => {:?}", first, alias);
                    Some((alias, v))
                } else {
                    None
                }
            }
        }
    }
    pub fn expand_alias<'a, T: 'a + AsRef<str>>(
        &'a self,
        args: &'a [T],
        conf: &'a Config,
    ) -> Option<Either<impl Iterator<Item = &'a str>, Vec<String>>> {
        if let Some((alias, remaining_args)) = self.find_alias(conf) {
            let (is_shell, after_args) = alias.args();
            let remaining_args = remaining_args[1..].iter().map(String::as_str);

            if is_shell {
                // shell 別名，完全無視開頭的參數（例如 `hs -s tag -H path/to/home`）
                let remaining_args = remaining_args.map(|s| to_display_args(s).to_string());
                let ret: Vec<_> = after_args
                    .map(ToOwned::to_owned)
                    .chain(remaining_args)
                    .collect();
                return Some(Either::Two(ret));
            }

            let base_len = args.len() - remaining_args.len() - 1;
            let base_args = args.iter().take(base_len).map(AsRef::as_ref);
            let new_args = base_args.chain(after_args).chain(remaining_args);

            // log::trace!("新的參數為 {:?}", new_args);
            Some(Either::One(new_args))
        } else {
            None
        }
    }
}

#[derive(Parser, Debug, Serialize)]
#[clap(disable_help_subcommand = true, args_override_self = true)]
pub enum Subs {
    #[clap(external_subcommand)]
    Other(Vec<String>),
    #[clap(
        about = "Prints this message, the help of the given subcommand(s), or a script's help message."
    )]
    Help { args: Vec<String> },
    #[clap(hide = true)]
    LoadUtils,
    #[clap(about = "Migrate the database")]
    Migrate,
    #[clap(about = "Edit hyper script", trailing_var_arg = true)]
    Edit {
        #[clap(long, short = 'T', help = TYPE_HELP)]
        ty: Option<ScriptFullType>,
        #[clap(long, short)]
        no_template: bool,
        #[clap(long, short, help = TAGS_HELP)]
        tags: Option<TagSelector>,
        #[clap(long, short, help = "Create script without invoking the editor")]
        fast: bool,
        #[clap(default_value = "?", help = EDIT_QUERY_HELP)]
        edit_query: Vec<EditQuery<ListQuery>>,
        #[clap(last = true)]
        content: Vec<String>,
    },
    #[clap(
        about = "Manage alias",
        disable_help_flag = true,
        allow_hyphen_values = true
    )]
    Alias {
        #[clap(
            long,
            short,
            requires = "before",
            conflicts_with = "after",
            help = "Unset an alias."
        )]
        unset: bool,
        before: Option<String>,
        #[clap(allow_hyphen_values = true)]
        after: Vec<String>,
    },

    #[clap(
        about = "Run the script",
        disable_help_flag = true,
        allow_hyphen_values = true
    )]
    Run {
        #[clap(long, help = "Add a dummy run history instead of actually running it")]
        dummy: bool,
        #[clap(long, short)]
        repeat: Option<u64>,
        #[clap(long, short, help = "Use arguments from last run")]
        previous: bool,
        #[clap(
            long,
            short = 'E',
            requires = "previous",
            help = "Raise an error if --previous is given but there is no previous run"
        )]
        error_no_previous: bool,
        #[clap(long, short, requires = "previous", help = "")]
        dir: Option<PathBuf>,
        #[clap(default_value = "-", help = SCRIPT_QUERY_HELP)]
        script_query: ScriptQuery,
        #[clap(
            help = "Command line args to pass to the script",
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    #[clap(about = "Execute the script query and get the exact file")]
    Which {
        #[clap(default_value = "-", help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
    },
    #[clap(about = "Print the script to standard output")]
    Cat {
        #[clap(default_value = "-", help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
        #[clap(long, help = "Read with other program, e.g. bat")]
        with: Option<String>,
    },
    #[clap(about = "Remove the script")]
    RM {
        #[clap(required = true, min_values = 1, help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
        #[clap(
            long,
            help = "Actually remove scripts, rather than hiding them with tag."
        )]
        purge: bool,
    },
    #[clap(about = "Set recent filter")]
    Recent { recent_filter: Option<Recent> },
    #[clap(about = "List hyper scripts")]
    LS(List),
    #[clap(about = "Manage script types")]
    Types(Types),
    #[clap(about = "Copy the script to another one")]
    CP {
        #[clap(long, short, help = TAGS_HELP)]
        tags: Option<TagSelector>,
        #[clap(help = SCRIPT_QUERY_HELP)]
        origin: ListQuery,
        #[clap(help = EDIT_CONCRETE_QUERY_HELP)]
        new: EditQuery<ScriptOrDirQuery>,
    },
    #[clap(about = "Move the script to another one")]
    MV {
        #[clap(long, short = 'T', help = TYPE_HELP)]
        ty: Option<ScriptType>,
        #[clap(long, short, help = TAGS_HELP)]
        tags: Option<TagSelector>,
        #[clap(help = LIST_QUERY_HELP)]
        origin: ListQuery,
        #[clap(help = EDIT_CONCRETE_QUERY_HELP)]
        new: Option<EditQuery<ScriptOrDirQuery>>,
    },
    #[clap(about = "Manage script tags")]
    Tags(Tags),
    #[clap(about = "Manage script history")]
    History {
        #[clap(subcommand)]
        subcmd: History,
    },
    #[clap(about = "Monitor hs process")]
    Top {
        #[clap(long, short, help = "Wait for all involved processes to halt")]
        wait: bool,
        #[clap(long, help = "Run event ID")]
        id: Vec<u64>,
        #[clap(help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub enum HistoryDisplay {
    Env,
    Args,
    All,
}
impl HistoryDisplay {
    pub fn show_args(&self) -> bool {
        match self {
            Self::Args | Self::All => true,
            Self::Env => false,
        }
    }
    pub fn show_env(&self) -> bool {
        match self {
            Self::Env | Self::All => true,
            Self::Args => false,
        }
    }
}
impl FromStr for HistoryDisplay {
    type Err = DisplayError;
    fn from_str(s: &str) -> DisplayResult<Self> {
        let g = match s {
            "env" => HistoryDisplay::Env,
            "args" => HistoryDisplay::Args,
            "all" => HistoryDisplay::All,
            _ => unreachable!(),
        };
        Ok(g)
    }
}

#[derive(Parser, Debug, Serialize)]
pub enum History {
    RM {
        #[clap(short, long)]
        dir: Option<PathBuf>, // FIXME: this flag isn't working...
        #[clap(long, possible_values(&["all", "env", "args"]), default_value = "args",)]
        display: HistoryDisplay,
        #[clap(long)]
        no_humble: bool,
        #[clap(required = true, min_values = 1, help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
        #[clap(last = true)]
        range: RangeQuery,
    },
    // TODO: 好想把它寫在 history rm 裡面...
    #[clap(
        name = "rm-id",
        about = "Remove an event by it's id.\nUseful if you want to keep those illegal arguments from polluting the history."
    )]
    RMID {
        event_id: u64,
    },
    #[clap(about = "Humble an event by it's id")]
    Humble {
        event_id: u64,
    },
    Show {
        #[clap(default_value = "-", help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
        #[clap(short, long, default_value = "10")]
        limit: u32,
        #[clap(long)]
        with_name: bool,
        #[clap(long)]
        no_humble: bool,
        #[clap(short, long, default_value = "0")]
        offset: u32,
        #[clap(short, long)]
        dir: Option<PathBuf>,
        #[clap(long, possible_values(&["all", "env", "args"]), default_value = "args",)]
        display: HistoryDisplay,
    },
    Neglect {
        #[clap(required = true, min_values = 1, help = LIST_QUERY_HELP)]
        queries: Vec<ListQuery>,
    },
    #[clap(disable_help_flag = true, allow_hyphen_values = true)]
    Amend {
        event_id: u64,
        #[clap(short, long)]
        env: Vec<EnvPair>,
        #[clap(long, conflicts_with = "env")]
        no_env: bool,
        #[clap(
            help = "Command line args to pass to the script",
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    Tidy,
}

#[derive(Parser, Debug, Serialize, Default)]
#[clap(args_override_self = true)]
pub struct List {
    // TODO: 滿滿的其它排序/篩選選項
    #[clap(short, long, help = "Show verbose information.")]
    pub long: bool,
    #[clap(long, possible_values(&["tag", "tree", "none"]), default_value = "tag", help = "Grouping style.")]
    pub grouping: Grouping,
    #[clap(long, help = "Limit the amount of scripts found.")]
    pub limit: Option<NonZeroUsize>,
    #[clap(long, help = "No color and other decoration.")]
    pub plain: bool,
    #[clap(long, help = "Show file path to the script.", conflicts_with = "long")]
    pub file: bool,
    #[clap(long, help = "Show name of the script.", conflicts_with = "long")]
    pub name: bool,
    #[clap(help = LIST_QUERY_HELP)]
    pub queries: Vec<ListQuery>,
}

fn set_home(p: &Option<String>, create_on_missing: bool) -> Result {
    path::set_home(p.as_ref(), create_on_missing)?;
    Config::init()
}

fn print_help<S: AsRef<str>>(cmds: impl IntoIterator<Item = S>) {
    // 從 clap 的 parse_help_subcommand 函式抄的，不曉得有沒有更好的做法
    let c = Root::command();
    let mut clap = &c;
    let mut had_found = false;
    for cmd in cmds {
        let cmd = cmd.as_ref();
        clap.find_subcommand(cmd);
        if let Some(c) = clap.find_subcommand(cmd) {
            clap = c;
            had_found = true;
        } else if !had_found {
            return;
        }
    }
    let _ = clap.clone().print_help();
    println!();
    std::process::exit(0);
}

macro_rules! map_clap_res {
    ($res:expr) => {{
        match $res {
            Err(err) => return Ok(ArgsResult::Err(err)),
            Ok(t) => t,
        }
    }};
}

fn handle_alias_args(args: Vec<String>) -> Result<ArgsResult> {
    match AliasRoot::try_parse_from(&args) {
        Ok(alias_root) if alias_root.root_args.no_alias => {
            log::debug!("不使用別名！");
            let root = map_clap_res!(Root::try_parse_from(args));
            return Ok(ArgsResult::Normal(root));
        }
        Ok(alias_root) => {
            log::info!("別名命令行物件 {:?}", alias_root);
            set_home(&alias_root.root_args.hs_home, true)?;
            let mut root = match alias_root.expand_alias(&args, Config::get()) {
                Some(Either::One(new_args)) => map_clap_res!(Root::try_parse_from(new_args)),
                Some(Either::Two(new_args)) => {
                    return Ok(ArgsResult::Shell(new_args));
                }
                None => map_clap_res!(Root::try_parse_from(&args)),
            };
            root.is_from_alias = true;
            Ok(ArgsResult::Normal(root))
        }
        Err(e) => {
            log::warn!(
                "解析別名參數出錯（應和 root_args 有關，如 --select 無值）：{}",
                e
            );
            map_clap_res!(Root::try_parse_from(args)); // NOTE: 不要讓這個錯誤傳上去，而是讓它掉入 Root::try_parse_from 中再來報錯
            unreachable!()
        }
    }
}

impl Root {
    /// 若帶了 --no-alias 選項，或是補全模式，我們可以把設定腳本之家（以及載入設定檔）的時間再推遲
    /// 在補全模式中意義重大，因為使用者可能會用 -H 指定別的腳本之家
    pub fn set_home_unless_from_alias(&self, create_on_missing: bool) -> Result {
        if !self.is_from_alias {
            set_home(&self.root_args.hs_home, create_on_missing)?;
        }
        Ok(())
    }
    pub fn sanitize_flags(&mut self, bang: bool) {
        if bang {
            self.root_args.timeless = true;
            self.root_args.select = vec!["all".parse().unwrap()];
        } else if self.root_args.all {
            self.root_args.timeless = true;
            self.root_args.select = vec!["all,^remove".parse().unwrap()];
        }
    }
    pub fn sanitize(&mut self) -> std::result::Result<(), ClapError> {
        match &mut self.subcmd {
            Some(Subs::Other(args)) => {
                let args = [APP_NAME, "run"]
                    .into_iter()
                    .chain(args.iter().map(|s| s.as_str()));
                self.subcmd = Some(Subs::try_parse_from(args)?);
                log::info!("執行模式 {:?}", self.subcmd);
            }
            Some(Subs::Help { args }) => {
                print_help(args.iter());
            }
            Some(Subs::Tags(tags)) => {
                tags.sanitize()?;
            }
            Some(Subs::Types(types)) => {
                types.sanitize()?;
            }
            None => {
                log::info!("無參數模式");
                self.subcmd = Some(Subs::Edit {
                    edit_query: vec![EditQuery::Query(ListQuery::Query(Default::default()))],
                    ty: None,
                    content: vec![],
                    tags: None,
                    fast: false,
                    no_template: false,
                });
            }
            _ => (),
        }
        self.sanitize_flags(false);
        Ok(())
    }
}

pub enum ArgsResult {
    Normal(Root),
    Completion(Completion),
    Shell(Vec<String>),
    Err(ClapError),
}

pub fn handle_args(args: Vec<String>) -> Result<ArgsResult> {
    if let Some(completion) = Completion::from_args(&args) {
        return Ok(ArgsResult::Completion(completion));
    }
    let mut root = handle_alias_args(args)?;
    if let ArgsResult::Normal(root) = &mut root {
        log::debug!("命令行物件：{:?}", root);
        map_clap_res!(root.sanitize());
    }
    Ok(root)
}

#[cfg(test)]
mod test {
    use super::*;
    fn try_build_args(args: &str) -> std::result::Result<Root, ClapError> {
        let v: Vec<_> = std::iter::once(APP_NAME)
            .chain(args.split(' '))
            .map(|s| s.to_owned())
            .collect();
        match handle_args(v).unwrap() {
            ArgsResult::Normal(root) => Ok(root),
            ArgsResult::Err(err) => Err(err),
            _ => panic!(),
        }
    }
    fn build_args(args: &str) -> Root {
        try_build_args(args).unwrap()
    }
    fn is_args_eq(arg1: &Root, arg2: &Root) -> bool {
        let json1 = serde_json::to_value(arg1).unwrap();
        let json2 = serde_json::to_value(arg2).unwrap();
        json1 == json2
    }
    #[test]
    fn test_strange_set_alias() {
        let args = build_args("alias trash -s remove");
        assert_eq!(args.root_args.select, vec![]);
        match &args.subcmd {
            Some(Subs::Alias {
                unset,
                after,
                before: Some(before),
            }) => {
                assert_eq!(*unset, false);
                assert_eq!(before, "trash");
                assert_eq!(after, &["-s", "remove"]);
            }
            _ => panic!("{:?} should be alias...", args),
        }
    }
    #[test]
    fn test_displaced_no_alias() {
        let ll = build_args("ll");
        assert!(!ll.root_args.no_alias);
        assert!(is_args_eq(&ll, &build_args("ls -l")));

        try_build_args("ll --no-alias").expect_err("ll 即 ls -l，不該有 --no-alias 作參數");

        let run_ll = build_args("--no-alias ll");
        assert!(run_ll.root_args.no_alias);
        assert!(is_args_eq(&run_ll, &build_args("--no-alias run ll")));

        let run_some_script = build_args("some-script --no-alias");
        assert!(!run_some_script.root_args.no_alias);
        let run_some_script_no_alias = build_args("--no-alias some-script");
        assert!(run_some_script_no_alias.root_args.no_alias);
    }
    #[test]
    fn test_strange_alias() {
        let args = build_args("-s e e -t e something -T e");
        assert_eq!(args.root_args.select, vec!["e".parse().unwrap()]);
        assert_eq!(args.root_args.all, false);
        match &args.subcmd {
            Some(Subs::Edit {
                edit_query,
                tags,
                ty,
                content,
                ..
            }) => {
                let query = match &edit_query[0] {
                    EditQuery::Query(ListQuery::Query(query)) => query,
                    _ => panic!(),
                };
                assert_eq!(query, &"something".parse().unwrap());
                assert_eq!(tags, &"e".parse().ok());
                assert_eq!(ty, &"e".parse().ok());
                assert_eq!(content, &Vec::<String>::new());
            }
            _ => {
                panic!("{:?} should be edit...", args);
            }
        }

        let args = build_args("la -l");
        assert_eq!(args.root_args.all, true);
        assert_eq!(args.root_args.select, vec!["all,^remove".parse().unwrap()]);
        match &args.subcmd {
            Some(Subs::LS(opt)) => {
                assert_eq!(opt.long, true);
                assert_eq!(opt.queries.len(), 0);
            }
            _ => {
                panic!("{:?} should be edit...", args);
            }
        }
    }
    #[test]
    fn test_multi_edit() {
        assert!(is_args_eq(
            &build_args("edit -- a b c"),
            &build_args("edit ? -- a b c")
        ));

        let args = build_args("edit a ? * -- x y z");
        match args.subcmd {
            Some(Subs::Edit {
                edit_query,
                content,
                ..
            }) => {
                assert_eq!(3, edit_query.len());
                assert!(matches!(
                    edit_query[0],
                    EditQuery::Query(ListQuery::Query(..))
                ));
                assert!(matches!(edit_query[1], EditQuery::NewAnonimous));
                assert!(matches!(
                    edit_query[2],
                    EditQuery::Query(ListQuery::Pattern(..))
                ));
                assert_eq!(
                    content,
                    vec!["x".to_owned(), "y".to_owned(), "z".to_owned()]
                );
            }
            _ => {
                panic!("{:?} should be edit...", args);
            }
        }
    }
    #[test]
    fn test_external_run_tags() {
        let args = build_args("-s test --dummy -r 42 =script -a --");
        assert!(is_args_eq(
            &args,
            &build_args("-s test run --dummy -r 42 =script -a --")
        ));
        assert_eq!(args.root_args.select, vec!["test".parse().unwrap()]);
        assert_eq!(args.root_args.all, false);
        match args.subcmd {
            Some(Subs::Run {
                dummy: true,
                previous: false,
                error_no_previous: false,
                repeat: Some(42),
                dir: None,
                script_query,
                args,
            }) => {
                assert_eq!(script_query, "=script".parse().unwrap());
                assert_eq!(args, vec!["-a", "--"]);
            }
            _ => {
                panic!("{:?} should be run...", args);
            }
        }

        let args = build_args("-s test --dump-args tags --name myname +mytag");
        assert!(is_args_eq(
            &args,
            &build_args("-s test --dump-args tags set --name myname +mytag")
        ));
        assert_eq!(args.root_args.select, vec!["test".parse().unwrap()]);
        assert_eq!(args.root_args.all, false);
        assert!(args.root_args.dump_args);
        match args.subcmd {
            Some(Subs::Tags(Tags {
                subcmd: Some(TagsSubs::Set { name, content }),
            })) => {
                assert_eq!(name, Some("myname".to_owned()));
                assert_eq!(content, "+mytag".parse().unwrap());
            }
            _ => {
                panic!("{:?} should be tags...", args);
            }
        }

        assert!(is_args_eq(
            &build_args("--humble"),
            &build_args("--humble edit -")
        ));
        assert!(is_args_eq(&build_args("tags"), &build_args("tags ls")));
    }
    #[test]
    fn test_disable_help() {
        let help_v = vec!["--help".to_owned()];
        let args = build_args("run =script --help");
        match args.subcmd {
            Some(Subs::Run {
                script_query, args, ..
            }) => {
                assert_eq!(script_query, "=script".parse().unwrap());
                assert_eq!(args, help_v);
            }
            _ => {
                panic!("{:?} should be run...", args);
            }
        }

        let args = build_args("alias a --help");
        match args.subcmd {
            Some(Subs::Alias { before, after, .. }) => {
                assert_eq!(before, Some("a".to_owned()));
                assert_eq!(after, help_v);
            }
            _ => {
                panic!("{:?} should be alias...", args);
            }
        }

        let args = build_args("history amend 42 --env A=1 --env B=2 --help");
        match args.subcmd {
            Some(Subs::History {
                subcmd:
                    History::Amend {
                        event_id,
                        args,
                        no_env,
                        env,
                    },
            }) => {
                assert_eq!(event_id, 42);
                assert_eq!(args, help_v);
                assert_eq!(no_env, false);
                assert_eq!(env, vec!["A=1".parse().unwrap(), "B=2".parse().unwrap()]);
            }
            _ => {
                panic!("{:?} should be history amend...", args);
            }
        }
    }
    #[test]
    #[ignore = "clap bug"]
    fn test_allow_hyphen() {
        assert!(is_args_eq(
            &build_args("alias a -u"),
            &build_args("alias a -- -u")
        ));
        assert!(is_args_eq(
            &build_args("run s --repeat 1"),
            &build_args("run s -- --repeat 1")
        ));
    }
}
