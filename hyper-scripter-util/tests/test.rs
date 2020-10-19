#[allow(dead_code)]
#[path = "../../hyper-scripter-test-lib/test_util.rs"]
mod test_util;

use hyper_scripter_util::get_all;
use std::fs::{create_dir_all, remove_dir_all, File};
use std::io::prelude::*;
use std::sync::MutexGuard;
use test_util::*;

pub fn setup_util<'a>() -> MutexGuard<'a, ()> {
    let g = setup();
    let utils = get_all();
    for u in utils.into_iter() {
        log::info!("載入 {}", u.name);
        run(&["e", "-c", u.category, u.name, u.content, "--no-template"]).unwrap();
    }
    g
}

fn test_import() {
    run(&[
        "e",
        "my/innate",
        "cp tests/to_be_import ./.tmp -r",
        "-f",
        "+innate",
    ])
    .unwrap();
    run(&["-f", "my", "-"]).unwrap();

    run(&["tags", "something-evil"]).unwrap();
    run(&["-f", "util", "import", ".tmp"]).unwrap();
    run(&["-f", "innate", "which", "myinnate"]).unwrap();

    assert_eq!(run(&["-f", "my", "test"]).unwrap(), "安安，紅寶石");
    assert_eq!(run(&["-f", "tag", "mytest"]).unwrap(), "安安，紅寶石");
    assert_eq!(run(&["-f", "tag", "youtest"]).unwrap(), "殼已破碎");
    assert_eq!(run(&["-f", "nameless", "-"]).unwrap(), "安安，匿名殼");

    run(&["-f", "something-evil", "which", "-"]).expect_err("標籤匯入錯了？");
    run(&["tags", "+all"]).unwrap();
}

const GITIGNORE_CONTENT: &'static str = ".script_history.db
*.db-*
.hs_exe_path
";
fn test_git() {
    run(&["-a", "git", "init"]).unwrap();
    assert_eq!(GITIGNORE_CONTENT, read(&[".gitignore"]));
}
fn test_collect() {
    create_dir_all(get_path().join("this/is/a/collect")).unwrap();
    let mut file = File::create(get_path().join("this/is/a/collect/test.rb")).unwrap();
    file.write_all("puts '這是一個收集測試'".as_bytes())
        .unwrap();
    remove_dir_all(get_path().join("my")).unwrap();
    run(&["-f", "innate", "which", "myinnate"]).expect("還沒跑 collect 就壞掉了？");
    run(&["-f", "my", "which", "mytest"]).expect("還沒跑 collect 就壞掉了？");
    run(&["thisisacolltest"]).expect_err("還沒收集就出現了，嚇死");

    run(&["collect"]).unwrap();
    assert_eq!(
        run(&["-f", "this", "thisisacolltest"]).unwrap(),
        "這是一個收集測試"
    );
    assert_eq!(
        run(&["-f", "is", "thisisacolltest"]).unwrap(),
        "這是一個收集測試"
    );
    run(&["-f", "innate", "which", "myinnate"]).expect_err("跑了 collect 沒有刪成功");
    run(&["-f", "my", "which", "mytest"]).expect_err("跑了 collect 沒有刪成功");

    assert_eq!(run(&["-f", "tag", "youtest"]).unwrap(), "殼已破碎");
    assert_eq!(run(&["-f", "nameless", "-"]).unwrap(), "安安，匿名殼");
}

#[test]
fn test_utils() {
    let _g = setup_util();
    test_import();
    test_git();
    test_collect();
}
