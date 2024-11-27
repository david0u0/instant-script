pub use hyper_scripter::path::get_home;
use hyper_scripter::{
    config::{Config, PromptLevel},
    error::EXIT_KNOWN_ERR,
    my_env_logger,
    path::normalize_path,
};
use shlex::Shlex;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Once;

pub const HOME_RELATIVE: &str = "./.hyper_scripter";

fn get_exe() -> String {
    #[cfg(not(debug_assertions))]
    let mode = "release";
    #[cfg(debug_assertions)]
    let mode = "debug";

    format!("{}/../target/{}/hs", env!("CARGO_MANIFEST_DIR"), mode)
}

fn get_editor_script() -> String {
    format!("{}/tests/editor.sh", env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug, Default)]
pub struct RunEnv {
    pub home: Option<PathBuf>,
    pub dir: Option<PathBuf>,
    pub only_touch: Option<String>,
    pub silent: Option<bool>,
    pub allow_other_error: Option<bool>,
    pub custom_env: Option<Vec<(String, String)>>,
}

macro_rules! run {
    ($($key:ident: $val:expr,)* $lit:literal) => ({
        run!($($key: $val,)* $lit,)
    });
    ($($key:ident: $val:expr,)* $lit:literal, $($arg:tt)*) => ({
        let env = RunEnv{
            $($key: Some($val.into()),)*
            ..Default::default()
        };
        run_with_env(env, format!($lit, $($arg)*))
    });
}
pub(crate) use run;

#[derive(Debug)]
enum ErrorInner {
    Other,
    ExitStatus(ExitStatus),
}
#[derive(Debug)]
pub struct Error {
    msg: Vec<String>,
    inner: ErrorInner,
}
impl Error {
    pub fn exit_status(e: ExitStatus) -> Error {
        Error {
            msg: vec![],
            inner: ErrorInner::ExitStatus(e),
        }
    }
    pub fn other<T: ToString>(s: T) -> Error {
        Error {
            msg: vec![s.to_string()],
            inner: ErrorInner::Other,
        }
    }
    pub fn context<T: ToString>(mut self, s: T) -> Error {
        self.msg.push(s.to_string());
        self
    }
}
type Result<T = ()> = std::result::Result<T, Error>;

fn get_test_home() -> PathBuf {
    let base = normalize_path(HOME_RELATIVE).unwrap();

    #[cfg(feature = "benching")]
    {
        return base;
    }

    let thread = std::thread::current();
    base.join(thread.name().unwrap_or("unnamed_thread"))
}
pub fn load_conf() -> Config {
    Config::load(get_home()).unwrap()
}
pub fn setup() -> () {
    let g = setup_with_utils();
    run!("rm --purge * -s all").unwrap();
    g
}
pub fn clean_and_set_home() {
    let _ = my_env_logger::try_init();
    let home = get_test_home();
    match std::fs::remove_dir_all(&home) {
        Ok(_) => (),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                panic!("重整測試用資料夾 {:?} 失敗了……", home);
            }
        }
    }

    // benchmark 時為了最佳效能，通常不會有 thread local home，故只在其它時候設定之
    #[cfg(feature = "benching")]
    {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            hyper_scripter::path::set_home(Some(home), true).unwrap();
        });
    }
    #[cfg(not(feature = "benching"))]
    {
        let p = Box::new(home);
        hyper_scripter::path::set_home_thread_local(Box::leak(p));
    }
}
pub fn setup_with_utils() -> () {
    clean_and_set_home();
    run!(silent: true, "ls").unwrap(); // create the home
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        Config::init().unwrap();
        Config::set_runtime_conf(Some(PromptLevel::Never), true);
    });

    // 避免編輯器堵住整個程式
    let mut conf = load_conf();
    conf.editor = vec!["bash".to_owned(), get_editor_script()];
    conf.store().unwrap();
    ()
}
fn join_path(p: &[&str]) -> PathBuf {
    let mut file = get_home().to_owned();
    for p in p.iter() {
        file = file.join(p);
    }
    file
}

pub fn read(p: &[&str]) -> String {
    let file = join_path(p);
    let s = std::fs::read(file).unwrap();
    let s: &str = std::str::from_utf8(&s).unwrap();
    s.to_owned()
}
pub fn check_exist(p: &[&str]) -> bool {
    let file = join_path(p);
    file.exists()
}

fn fmt_result(res: &Result<String>) -> String {
    if let Ok(s) = res.as_ref() {
        format!("\n{}", s)
    } else {
        format!("{:?}", res)
    }
}

pub fn run_with_env<T: ToString>(env: RunEnv, args: T) -> Result<String> {
    let home = match &env.home {
        Some(h) => {
            log::info!("使用腳本之家 {:?}", h);
            &*h
        }
        None => get_home(),
    };
    let home = home.to_string_lossy();
    let mut full_args: Vec<_> = ["-H", home.as_ref(), "--prompt-level", "never"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let args = args.to_string();
    if let Some((first, second)) = args.split_once("|") {
        full_args.extend(Shlex::new(&first));
        let second = second.trim();
        if second.len() > 0 {
            full_args.push("--".to_owned());
            full_args.push(second.to_owned());
        }
    } else {
        full_args.extend(Shlex::new(&args))
    };

    run_cmd(normalize_path(get_exe()).unwrap(), &full_args, env)
}

pub fn run_cmd(
    exe: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr> + Debug],
    env: RunEnv,
) -> Result<String> {
    log::info!("開始執行 {:?}", args);
    let mut cmd = Command::new(exe);
    if let Some(dir) = env.dir {
        log::info!("使用路徑 {}", dir.to_string_lossy());
        cmd.current_dir(&dir);
        // cmd.env("PWD", dir); NOTE: 不應使用 PWD 環境變數
    }
    if let Some(only_touch) = env.only_touch {
        log::info!("only touch {}", only_touch);
        cmd.env("ONLY_TOUCH", only_touch);
    }
    if let Some(custom_env) = env.custom_env {
        for (k, v) in custom_env.iter() {
            cmd.env(k, v);
        }
    }
    let mut child = cmd
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.as_mut().unwrap();
    let mut out_str = vec![];
    let reader = BufReader::new(stdout);
    let silent = env.silent;
    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| {
            if silent != Some(true) {
                println!("{}", line);
            }
            out_str.push(line);
        });

    let status = child.wait().unwrap();
    let res = if status.success() {
        Ok(out_str.join("\n"))
    } else if env.allow_other_error == Some(true) || status.code() == Some(EXIT_KNOWN_ERR.code()) {
        Err(Error::exit_status(status).context(format!("執行 {:?} 失敗", args)))
    } else {
        panic!("執行 {:?} 遭未知的錯誤！", args);
    };
    log::info!("執行 {:?} 完畢，結果為 {}", args, fmt_result(&res));
    res
}

pub fn get_ls(select: Option<&str>, query: Option<&str>) -> Vec<String> {
    let ls_res = run!(
        "ls {} --grouping none --plain --name {}",
        select.map(|f| format!("-s {}", f)).unwrap_or_default(),
        query.unwrap_or_default()
    )
    .unwrap();
    ls_res
        .split_whitespace()
        .filter_map(|s| {
            if !s.is_empty() {
                Some(s.to_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}
pub fn assert_ls_len(expect: usize, select: Option<&str>, query: Option<&str>) {
    let res = get_ls(select, query);
    assert_eq!(expect, res.len(), "ls {:?} 結果為 {:?}", select, res);
}
pub fn assert_ls<T: ToString>(expect: Vec<T>, select: Option<&str>, query: Option<&str>) {
    let mut expect: Vec<_> = expect.into_iter().map(|s| s.to_string()).collect();
    expect.sort();
    let mut res = get_ls(select, query);
    res.sort();
    assert_eq!(expect, res, "ls {:?} 結果為 {:?}", select, res);
}

// TODO: 把整合測試的大部份地方改用這個結構
#[derive(Debug)]
pub struct ScriptTest {
    name: String,
}
impl<'a> ToString for &'a ScriptTest {
    fn to_string(&self) -> String {
        self.name.clone()
    }
}
impl ScriptTest {
    pub fn get_name(&self) -> &str {
        &self.name
    }
    pub fn new_regardless(name: &str, tags: Option<&str>, content: Option<&str>) -> (Self, Result) {
        let tags_str = tags.map(|s| format!("-t {}", s)).unwrap_or_default();
        let content = content.unwrap_or("echo $NAME");
        let res = run!("e {} ={} | {}", tags_str, name, content).map(|_| ());
        (
            ScriptTest {
                name: name.to_owned(),
            },
            res,
        )
    }
    pub fn new(name: &str, tags: Option<&str>, content: Option<&str>) -> Self {
        let (t, res) = Self::new_regardless(name, tags, content);
        res.unwrap();
        t
    }
    pub fn assert_not_exist(&self, args: Option<&str>, msg: Option<&str>) {
        let s = format!("cat {} ={}", args.unwrap_or_default(), self.name);
        let msg = msg.map(|s| format!("\n{}", s)).unwrap_or_default();
        run!("{}", s).expect_err(&format!("{} 找到東西{}", s, msg));
    }
    pub fn archaeology<'a>(&'a self) -> ScriptTestWithSelect<'a> {
        self.select("-A")
    }
    pub fn select<'a>(&'a self, select: &'a str) -> ScriptTestWithSelect<'a> {
        ScriptTestWithSelect {
            script: self,
            allow_other_error: false,
            select,
        }
    }
    pub fn allow_other_error<'a>(&'a self) -> ScriptTestWithSelect<'a> {
        ScriptTestWithSelect {
            script: self,
            allow_other_error: true,
            select: "",
        }
    }
    pub fn run(&self, args: &str) -> Result<String> {
        self.select("").run(args)
    }
    pub fn can_find(&self, command: &str) -> Result {
        self.select("").can_find(command)
    }
    pub fn can_find_by_name(&self) -> Result {
        self.select("").can_find_by_name()
    }
}
pub struct ScriptTestWithSelect<'a> {
    script: &'a ScriptTest,
    select: &'a str,
    allow_other_error: bool,
}
impl<'a> ScriptTestWithSelect<'a> {
    pub fn run(&self, args: &str) -> Result<String> {
        run!(
            allow_other_error: self.allow_other_error,
            "{} ={} {}",
            self.select,
            self.script.name,
            args
        )
    }
    pub fn can_find(&self, command: &str) -> Result {
        let res = run!(
            allow_other_error: self.allow_other_error,
            "{} ls --plain --grouping=none --name {}",
            self.select,
            command
        )?;
        if res == self.script.name {
            Ok(())
        } else {
            Err(Error::other(format!(
                "想找 {} 卻找到 {}",
                self.script.name, res
            )))
        }
    }
    pub fn can_find_by_name(&self) -> Result {
        self.can_find(&format!("={}", self.script.name))
    }
}
