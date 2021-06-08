use hyper_scripter::config::{Config, PromptLevel, RawConfig};
use std::fs::canonicalize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

lazy_static::lazy_static! {
    static ref LOCK: Mutex<()> = Mutex::new(());
}
#[cfg(not(debug_assertions))]
const EXE: &'static str = "../target/release/hs";
#[cfg(debug_assertions)]
const EXE: &str = "../target/debug/hs";

pub fn get_exe_abs() -> String {
    canonicalize(EXE)
        .unwrap()
        .to_string_lossy()
        .as_ref()
        .to_owned()
}

const HOME: &str = "./.hyper_scripter";

pub fn get_home() -> PathBuf {
    canonicalize(HOME).unwrap()
}
pub fn load_conf() -> Config {
    RawConfig::load().unwrap().unwrap().0.into()
}
pub fn setup<'a>() -> MutexGuard<'a, ()> {
    let g = setup_with_utils();
    run("rm --purge * -f all").unwrap();
    g
}
pub fn setup_with_utils<'a>() -> MutexGuard<'a, ()> {
    let guard = LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let _ = env_logger::try_init();
    let home: PathBuf = HOME.into(); // 不要想用 get_home，因為 canonicalize 若路徑不存在就會炸裂
    match std::fs::remove_dir_all(&home) {
        Ok(_) => (),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                panic!("重整測試用資料夾 {:?} 失敗了……", home);
            }
        }
    }

    hyper_scripter::path::set_home(&home).unwrap();

    run("alias e edit --fast").unwrap();
    let mut conf = load_conf();
    conf.prompt_level = PromptLevel::Never;
    conf.store().unwrap();

    guard
}
fn join_path(p: &[&str]) -> PathBuf {
    let mut file = get_home();
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
pub fn run<T: ToString>(args: T) -> Result<String, ExitStatus> {
    let home = get_home();
    run_with_home(&home, args)
}
pub fn split_args<'a>(s: &'a str, home: &'a Path) -> Vec<&'a str> {
    let mut full_args = vec!["-H", home.to_str().unwrap()];
    let args_vec: Vec<&str> = if s.find('|').is_some() {
        let (first, second) = s.split_once("|").unwrap();
        let mut v: Vec<_> = first.split(' ').filter(|s| !s.is_empty()).collect();
        v.push(second.trim());
        v
    } else {
        s.split_whitespace().collect()
    };
    full_args.extend(&args_vec);
    full_args
}
pub fn run_with_home<T: ToString>(home: &Path, args: T) -> Result<String, ExitStatus> {
    let args = args.to_string();
    let args = split_args(&args, home);
    let mut cmd = Command::new(EXE);
    let mut child = cmd
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.as_mut().unwrap();
    let mut out_str = vec![];
    let reader = BufReader::new(stdout);
    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| {
            println!("{}", line);
            out_str.push(line);
        });

    let status = child.wait().unwrap();
    let res = if status.success() {
        Ok(out_str.join("\n"))
    } else {
        Err(status)
    };
    log::info!("執行 {:?} 完畢，結果為 {:?}", args, res);
    res
}

fn get_ls(filter: Option<&str>, query: Option<&str>) -> Vec<String> {
    let ls_res = run(format!(
        "ls {} --grouping none --plain --name {}",
        filter.map(|f| format!("-f {}", f)).unwrap_or_default(),
        query.unwrap_or_default()
    ))
    .unwrap();
    ls_res
        .split(' ')
        .filter_map(|s| {
            if !s.is_empty() {
                Some(s.to_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}
pub fn assert_ls_len(expect: usize, filter: Option<&str>, query: Option<&str>) {
    let res = get_ls(filter, query);
    assert_eq!(expect, res.len(), "ls {:?} 結果為 {:?}", filter, res);
}
pub fn assert_ls(mut expect: Vec<&str>, filter: Option<&str>, query: Option<&str>) {
    expect.sort_unstable();
    let mut res = get_ls(filter, query);
    res.sort();
    assert_eq!(expect, res, "ls {:?} 結果為 {:?}", filter, res);
}
