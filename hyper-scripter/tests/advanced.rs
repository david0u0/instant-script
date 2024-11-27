#[allow(dead_code)]
#[path = "tool.rs"]
mod tool;
pub use tool::*;

use hyper_scripter::{
    script_type::ScriptFullType,
    util::{remove, write_file},
};
use std::path::PathBuf;

#[test]
fn test_mv_dir() {
    let _g = setup();
    ScriptTest::new("file1", Some("remove"), None);
    ScriptTest::new("dir1/file1", None, None);
    ScriptTest::new("dir1/dir2/file1", None, None);
    ScriptTest::new("dir3/native", None, None);
    ScriptTest::new("dirless", None, None);

    run!("mv dir1/* dir3/ -t hide").expect_err("做為移動目標的兩個 file1 撞名");

    run!("mv d1d2 dir1/dir2/file2").unwrap();
    run!("mv dir1/* dir3/ -t hide").expect("不再撞名");
    assert_ls(vec!["dir3/native", "dirless"], None, None);
    run!("tags all,^remove").unwrap();
    assert_ls(
        vec!["dir3/file1", "dir3/file2", "dir3/native", "dirless"],
        None,
        None,
    );

    run!("mv dir3/file* /").expect_err("移動目標與既存的 file1 撞名");

    run!("mv file1! file4").unwrap();
    run!("mv dir3/file* /").expect("不再撞名");
    assert_ls(
        vec!["file1", "file2", "file4", "dir3/native", "dirless"],
        Some("all"),
        None,
    );
}

// TODO: test cp

#[test]
fn test_sub_tmpl() {
    let _g = setup();
    fn get_types_vec() -> Vec<String> {
        let mut v: Vec<_> = run!("types ls")
            .unwrap()
            .split_ascii_whitespace()
            .map(|s| s.to_owned())
            .collect();
        v.sort();
        v
    }
    fn modify_types_vec(og: &mut Vec<String>, new: &[&str], rmed: &[&str]) {
        og.retain(|s| !rmed.contains(&s.as_ref()));
        og.extend(new.iter().map(|s| s.to_string()));
        og.sort();
    }
    fn get_tmpl_path(name: &str) -> PathBuf {
        let ty: ScriptFullType = name.parse().unwrap();
        hyper_scripter::path::get_template_path(&ty).unwrap()
    }

    const RB_TRAVERSE: &str = "rb/traverse";
    const JS_SUPER_WEIRD_NAME: &str = "js/suPeR-weIRd_NAme";

    run!("e traverse-test -T {} | puts 'test!'", RB_TRAVERSE).unwrap();

    let mut types = get_types_vec();
    let p = get_tmpl_path(RB_TRAVERSE);
    remove(&p).unwrap();
    run!("e ? -T {} | puts 'test!'", RB_TRAVERSE).expect_err("子模版被砍了就不該創新的");
    assert_ls_len(1, None, None);

    assert_ne!(get_types_vec(), types); // rb/traverse 還在向量中
    modify_types_vec(&mut types, &[], &[RB_TRAVERSE]);
    assert_eq!(get_types_vec(), types);

    assert_eq!(
        run!("types js").unwrap(),
        run!("types {}", JS_SUPER_WEIRD_NAME).unwrap(),
        "子模版的預設值應該和父類別相同（除非是寫死的那幾個，如 rb/traverse）"
    );
    let p = get_tmpl_path(JS_SUPER_WEIRD_NAME);
    write_file(
        &p,
        "console.log('hello from {{name}}');{{{content.0}}};console.log({{{content.1}}} + 5);",
    )
    .unwrap();
    assert_ne!(
        run!("types js").unwrap(),
        run!("types {}", JS_SUPER_WEIRD_NAME).unwrap(),
        "子模版檔案已被寫入，不該相同"
    );

    assert_ne!(get_types_vec(), types); // 怪名字還不在向量中
    modify_types_vec(&mut types, &[JS_SUPER_WEIRD_NAME], &[]);
    assert_eq!(get_types_vec(), types);

    run!(
        "e weird-test -T {} -f -- 'let a = 78' 'a + 4'",
        JS_SUPER_WEIRD_NAME
    )
    .unwrap();
    assert_eq!("hello from weird-test\n87", run!("weird-test").unwrap());
    run!("traverse-test").expect("刪個子模版不應影響已存在的腳本！");
}

#[test]
fn test_bang_list_query() {
    let _g = setup();
    let a = ScriptTest::new("dir/a", Some("hide"), None);
    let b = ScriptTest::new("dir/b", Some("hide"), None);
    let c = ScriptTest::new("dir/c", Some("hide"), None);
    let d = ScriptTest::new("dir/d", None, None);
    let e = ScriptTest::new("e", None, None);

    assert_ls(vec![&d], None, Some("dir/*"));
    assert_ls(vec![&d, &e], None, Some("*"));
    assert_ls(vec![&a, &b, &c, &d], None, Some("dir/*!"));
    run!("ls dir2/*!").expect_err("不存在的列表查詢還是得報錯");
}

#[test]
fn test_type_select() {
    let _g = setup();
    let a_rb = "a";
    let b = "b";
    let c_rb = "c";
    let d = "d";
    let e_hide_rb = "e";
    let f_tag_rb = "f";

    run!("e -f {}", b).unwrap();
    run!("e -f {}", d).unwrap();
    run!("e -f -T rb {}", a_rb).unwrap();
    run!("e -f -T rb {}", c_rb).unwrap();
    run!("e -f -t hide -T rb {}", e_hide_rb).unwrap();
    run!("e -f -t tag -T rb {}", f_tag_rb).unwrap();

    assert_ls(vec![a_rb, b, c_rb, d, f_tag_rb], None, None);
    assert_ls(vec![b, d], Some("@sh"), None);
    assert_ls(vec![b, d, f_tag_rb], Some("@sh,tag"), None);

    assert_ls(vec![a_rb, c_rb, f_tag_rb], Some("+^@sh"), None);
    assert_ls(vec![a_rb, c_rb], Some("+^@sh,^tag"), None);

    assert_ls(vec![a_rb, c_rb, e_hide_rb, f_tag_rb], Some("@rb"), None);
    assert_ls(vec![a_rb, b, c_rb, d, f_tag_rb], Some("+@rb"), None);

    assert_ls(vec![a_rb, c_rb, f_tag_rb], Some("+@rb!"), None);
}

#[test]
fn test_prev_env() {
    let _g = setup();
    const MY_ENV: &str = "MY_ENV";
    const MY_OTHER_ENV: &str = "MY_OTHER_ENV";
    const MY_ENV_HELP: &str = "MY_ENV_HELP";
    run!(
        "e --no-template ? | 
        # [HS_ENV]: {}
        # [HS_ENV]: {}
        # [HS_ENV_HELP]: {}
        echo ${}:${}:${}
        ",
        MY_ENV,
        MY_OTHER_ENV,
        MY_ENV_HELP,
        MY_ENV,
        MY_OTHER_ENV,
        MY_ENV_HELP,
    )
    .unwrap();

    let env = vec![
        (MY_ENV.to_owned(), "A".to_owned()),
        (MY_ENV_HELP.to_owned(), "C".to_owned()),
    ];

    assert_eq!(run!("-").unwrap(), "::");
    assert_eq!(run!("run -p").unwrap(), "::");

    assert_eq!(run!(custom_env: env, "run -p").unwrap(), "A::C");
    assert_eq!(run!("run -p").unwrap(), "A::");
    assert_eq!(run!("run -p").unwrap(), "A::");
    assert_eq!(run!("run -p").unwrap(), "A::");

    let env = vec![
        (MY_ENV.to_owned(), "X".to_owned()),
        (MY_OTHER_ENV.to_owned(), "B".to_owned()),
    ];
    // -p is stronger than normal env var
    assert_eq!(run!(custom_env: env.clone(), "run -p").unwrap(), "A:B:");
    assert_eq!(run!("run -p").unwrap(), "A:B:");

    assert_eq!(run!(custom_env: env, "run -").unwrap(), "X:B:");
    assert_eq!(run!("run -p").unwrap(), "X:B:");

    assert_eq!(run!("-").unwrap(), "::");
    assert_eq!(run!("run -p").unwrap(), "::");
}

#[test]
fn test_shell_alias() {
    let _g = setup();

    run!("alias test-alias !echo a").unwrap();
    assert_eq!("a", run!("test-alias").unwrap());
    assert_eq!("a b", run!("test-alias b").unwrap());

    assert_eq!("a", run!("-a test-alias").unwrap());
    assert_eq!("a -a", run!("test-alias -a").unwrap());

    run!(allow_other_error: true, "-s inva!id test-alias").expect_err("invalid args");
    run!("-s valid test-alias").expect("valid args");

    run!("alias -u test-alias").unwrap();
    run!("test-alias").expect_err("alias is unset!");

    // env in shell alias
    const MSG: &'static str = "this is a test";
    run!("e -T rb that-file | puts \"#{{ENV['HS_HOME']}}: {}\"", MSG).unwrap();
    run!("e -T txt this-file | {}", MSG).unwrap();
    run!("alias readit !cat $HS_HOME/this-file").unwrap();
    assert_eq!(MSG, run!("cat").unwrap());
    assert_eq!(MSG, run!("readit").unwrap());

    // escape character e.g. "*"
    run!("alias lsit !$HS_EXE -H $HS_HOME ls").unwrap();
    assert_eq!(run!("ls").unwrap(), run!(dir: "/", "lsit *").unwrap());

    run!("alias with '!cd $HS_HOME;'").unwrap();
    let home = get_home().to_string_lossy();
    let file_path = format!("{}/that-file.rb", home);
    let expected = format!("{}: {}", home, MSG);
    assert_eq!(expected, run!("that-file").unwrap());
    assert_eq!(expected, run!("with ruby ./that-file.rb").unwrap());
    assert_ne!(
        expected,
        run_cmd("ruby", &[file_path], Default::default()).unwrap()
    ); // run the script without hs, should not have env variables
}

#[test]
fn test_special_anonymous_query() {
    let _g = setup();

    let s1 = ScriptTest::new(".1", None, None);
    let s2 = ScriptTest::new(".10", None, None);
    let s3 = ScriptTest::new("1.a", None, None);
    let s4 = ScriptTest::new("1/a", None, None);

    s1.run("").unwrap();
    s1.can_find(".").unwrap();
    s1.can_find("1").unwrap();

    s2.run("").unwrap();
    s2.can_find(".").unwrap();
    s1.can_find("1").unwrap();

    s3.run("").unwrap();
    s2.can_find(".").unwrap();
    s3.can_find("1").unwrap();

    s4.run("").unwrap();
    s2.can_find(".").unwrap();
    s4.can_find("1").unwrap();

    run!("rm *").unwrap();
    let s1 = ScriptTest::new("a.b", None, None);
    let s2 = ScriptTest::new("a.bc", None, None);

    s1.run("").unwrap();
    s1.can_find(".").unwrap();

    s2.run("").unwrap();
    s1.can_find(".").expect("非匿名腳本不應觸發特殊規則");
}

#[test]
fn test_cat_with() {
    let _g = setup();

    let content = "# first AAA line\necho AAA";
    run!("e -T txt | # first\necho BBB").unwrap();
    run!("e -T txt | {}", content).unwrap();

    assert_ne!("AAA", run!("cat").unwrap());
    assert_eq!(content, run!("cat").unwrap());
    assert_eq!("AAA", run!("cat --with=sh").unwrap());

    let simple_grep = run!("cat --with='grep AAA'").unwrap();
    assert_eq!(content, simple_grep);

    let complex_grep = run!("cat --with='grep \"AAA line\"'").unwrap();
    assert_ne!(content, complex_grep);
    assert_eq!("# first AAA line", complex_grep,);

    let multi_grep = run!("cat --with='grep -h echo' *").unwrap();
    assert_eq!("echo AAA\necho BBB", multi_grep);
}
