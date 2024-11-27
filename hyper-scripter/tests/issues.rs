#[allow(dead_code)]
#[path = "tool.rs"]
mod tool;

use hyper_scripter::script_type::ScriptType;
use std::fs::remove_file;
use tool::*;

#[test]
fn test_mv_same_name() {
    println!("移動腳本時，若前後腳本的路徑相同（腳本分類所致），應順利改動");
    let _g = setup();

    run!("e test1 | echo 1").unwrap();
    run!("e test2 | echo 2").unwrap();

    run!("mv test1 -T tmux").unwrap();
    assert_eq!(run!("test1").unwrap(), "1");

    run!("mv test1 test2").expect_err("移動成撞名的腳本應報錯");
    assert_eq!(run!("test1").unwrap(), "1");
    run!("mv test1 test2 -T rb").expect_err("移動成撞名的腳本，即使路徑不同，也應報錯");
    assert_eq!(run!("test1").unwrap(), "1");
}
#[test]
fn test_cp_same_name() {
    println!("複製腳本時，若和既存的腳本撞名，應報錯");
    let _g = setup();

    run!("e test1 -T rb | puts 1").unwrap();
    run!("e test2 | echo 2").unwrap();
    run!("e test3 -T rb | puts 3").unwrap();

    run!("cp test1 test3").expect_err("複製成撞名的腳本應報錯");
    assert_eq!(run!("test3").unwrap(), "3");

    run!("cp test1 test2").expect_err("複製成撞名的腳本，即使路徑不同，也應報錯");
    assert_eq!(run!("test2").unwrap(), "2");

    assert_eq!(run!("test1").unwrap(), "1");
}
#[test]
fn test_rm_non_exist() {
    println!("若欲刪除的腳本不存在，應直接消滅之。");
    let _g = setup();

    run!("e test1 | echo '刪我啊'").unwrap();
    run!("e test2 | echo '別刪我QQ'").unwrap();
    assert_eq!(run!("test1").unwrap(), "刪我啊");
    let file = get_home().join("test1.sh");
    remove_file(&file).unwrap();
    assert_ls_len(2, None, None);
    assert_eq!(run!("which test1").unwrap(), file.to_string_lossy()); // TODO: 應該允許 which 嗎？

    run!("test1").expect_err("刪掉的腳本還能執行！？");
    run!("rm *").expect("rm 應該要消滅掉不存在的腳本");

    assert_ls_len(1, Some("all"), None);
}
#[test]
fn test_hs_in_hs() {
    println!("若腳本甲裡呼叫了本程式去執行腳本乙，完成之後腳本甲的時間應較新");
    let _g = setup();

    let outer = ScriptTest::new("outer", None, Some("$HS_EXE -H $HS_HOME inner"));
    let inner = ScriptTest::new("inner", None, Some("echo 我在第幾層？"));

    inner.can_find("^2").unwrap_err();
    outer.run("").unwrap();
    inner.can_find("^2").unwrap();

    outer.select("--humble").run("").unwrap();
    inner.can_find("^2").unwrap_err();

    let outer_humble = ScriptTest::new(
        "outer-humble",
        None,
        Some("$HS_EXE -H $HS_HOME history humble $HS_RUN_ID && $HS_EXE -H $HS_HOME inner"),
    );

    inner.can_find("-").unwrap_err();
    outer_humble.run("").unwrap();
    inner.can_find("-").unwrap();
}

#[test]
fn test_remove_history_in_script() {
    println!("在腳本執行途中砍掉執行歷史，則該腳本的「執行完畢」事件應該一併消失");
    let _g = setup();

    run!("e test1 | echo 1").unwrap();
    run!("e test2 | echo 2 && $HS_EXE -H $HS_HOME history rm =${{NAME}}! -- 1").unwrap();

    assert_eq!(run!("-").unwrap(), "2");
    assert_eq!(run!("-").unwrap(), "2"); // 比較晚創造，所以刪了執行事件還是腳本2先
    assert_eq!(run!("test1").unwrap(), "1");
    assert_eq!(run!("test2").unwrap(), "2");
    assert_eq!(run!("-").unwrap(), "1");
}

#[test]
fn test_edit_existing_bang() {
    println!("用 BANG! 編輯已存在的腳本，不該出錯");
    let _g = setup();

    run!("e test -t hide | echo 躲貓貓").unwrap();
    run!("e -t gg non-hidden -T rb | puts '光明正大'").unwrap();

    // 當場變一個異步執行期出來。不要直接把測試函式寫成異步，否則 setup 中鎖的處理會出問題…
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        use hyper_scripter::error::{Error, RedundantOpt};
        use hyper_scripter::script_repo::ScriptRepo;
        use hyper_scripter::tag::{Tag, TagSelector};
        use hyper_scripter::util::{
            init_env,
            main_util::{edit_or_create, EditTagArgs},
        };

        let mut repo = {
            let (env, _) = init_env(true).await.unwrap();
            let group = "all,^hide".parse::<TagSelector>().unwrap().into();
            ScriptRepo::new(Default::default(), env, &group)
                .await
                .unwrap()
        };

        fn assert_tags<'a>(expect: &[&str], actual: impl Iterator<Item = &'a Tag>) {
            let expect: Vec<Tag> = expect.iter().map(|s| s.parse().unwrap()).collect();
            let actual: Vec<Tag> = actual.map(|s| s.clone()).collect();
            assert_eq!(expect, actual);
        }
        macro_rules! try_edit {
            ($query:expr, $ty:expr, $tag:expr) => {
                edit_or_create(
                    vec![$query.parse().unwrap()],
                    &mut repo,
                    $ty.map(|s: &str| s.parse().unwrap()),
                    EditTagArgs {
                        content: $tag.parse().unwrap(),
                        explicit_tag: false,
                        explicit_select: false,
                    },
                )
            };
        }

        let err = try_edit!("test", Some("rb"), "gg")
            .await
            .expect_err("沒有 BANG! 就找到編輯的腳本！？");
        assert!(matches!(err, Error::ScriptIsFiltered(s) if s == "test"));

        let err = try_edit!("test", None, "gg").await.unwrap_err();
        assert!(matches!(err, Error::ScriptIsFiltered(s) if s == "test"));

        let err = try_edit!("test!", Some("rb"), "gg").await.unwrap_err();
        assert!(matches!(err, Error::RedundantOpt(RedundantOpt::Type)));

        // name different from `test.rb`, so we can create it
        let (edit, create) = try_edit!("tes", Some("rb/traverse"), "+gg").await.unwrap();
        let create = create.unwrap();
        assert!(edit.existing.is_empty());
        assert_eq!(
            vec![get_home().join("tes.rb")],
            create.iter_path().collect::<Vec<_>>()
        );
        assert!(create.ty.sub.is_some());

        // edit_or_create 不會改變 repo 的狀態，所以重複創造也不會報錯
        let (edit, create) = try_edit!("tes", Some("rb"), "+gg").await.unwrap();
        assert!(create.is_some());
        assert!(edit.existing.is_empty());

        // simple edit, should find existing script `non-hidden.rb`
        let (edit, create) = try_edit!("non-hi", None, "+zzzz").await.unwrap();
        assert!(create.is_none());
        let entry = &edit.existing[0];
        assert_tags(&["gg"], entry.tags.iter());

        let err = try_edit!("non-hi", Some("rb/cd"), "+zzzz")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::RedundantOpt(RedundantOpt::Type)));

        // edit exact name, so create new script `non-hi.rb`
        let (edit, create) = try_edit!("=non-hi", Some("rb/cd"), "+zzzz").await.unwrap();
        let create = create.unwrap();
        assert!(edit.existing.is_empty());
        assert_eq!(
            vec![get_home().join("non-hi.rb")],
            create.iter_path().collect::<Vec<_>>()
        );
        assert!(create.ty.sub.is_some());
        assert_tags(&["zzzz"], create.tags.iter());

        // simple edit and name is disjoint from others, create script `test2.sh`
        let (edit, create) = try_edit!("test2", None, "+gg").await.unwrap();
        let create = create.unwrap();
        assert!(edit.existing.is_empty());
        assert_eq!(
            vec![get_home().join("test2.sh")],
            create.iter_path().collect::<Vec<_>>()
        );
        assert!(create.ty.sub.is_none());
        assert_tags(&["gg"], create.tags.iter());

        let (edit, create) = try_edit!("test!", None, "+a,^b,c").await.unwrap();
        assert!(create.is_none());
        let entry = &edit.existing[0];
        assert_tags(&["hide"], entry.tags.iter());
    });
}

// TODO: edit wild & edit phantom

#[test]
fn test_edit_without_change() {
    println!("沒有動到檔案就不要記錄寫入事件");
    let _g = setup();

    const ORPHAN: &str = "orphan";
    run!(only_touch: "", "e {}", ORPHAN).expect_err("空編輯應該是一個錯誤");
    assert_ls_len(0, Some("all"), None);
    run!(only_touch: "", "e {} | this is a test", ORPHAN).expect_err("帶內容不存檔，仍視為未編輯");
    assert_ls_len(0, Some("all"), None);

    run!(only_touch: "yes1;yes2", "e yes1 no1 yes2 no2").unwrap_err();
    assert_ls_len(2, Some("all"), None);
    run!("which yes1 yes2").unwrap();

    let t = ScriptTest::new("target", None, None);
    let base = ScriptTest::new("baseline", None, None);
    base.can_find("!").unwrap();
    run!("history neglect {}", t.get_name()).unwrap();
    t.can_find_by_name().expect_err("被忽略還找得到？");
    run!(only_touch: "", "e {}!", t.get_name()).unwrap(); // nothing changed!
    t.can_find_by_name().expect_err("空編輯不應打破時間篩選");

    base.can_find("-").unwrap();
    t.can_find("!").unwrap();

    run!("e {}! | echo $NAME", t.get_name()).unwrap(); // changed!
    t.can_find_by_name().unwrap();
    assert_eq!(
        format!("{}\n{}", t.get_name(), t.get_name()),
        run!("-").unwrap(),
        "帶內容編輯應打破時間篩選"
    );
}

#[test]
fn test_multifuzz() {
    use hyper_scripter::{fuzzy::*, SEP};

    println!("模糊搜撞在一起時的特殊行為 1. 取最新者 2. 不可為「正解」的後綴");
    let _g = setup();
    let pref = ScriptTest::new("multifuzz", None, None);
    let t1 = ScriptTest::new("multifuzz/t1", None, None);
    let t2 = ScriptTest::new("multifuzz/t2", None, None);

    // 當場變一個異步執行期出來。不要直接把測試函式寫成異步，否則 setup 中鎖的處理會出問題…
    let rt = tokio::runtime::Runtime::new().unwrap();
    let res = rt.block_on(async {
        fuzz("mult", [&t1, &t2, &pref].iter().map(|t| t.get_name()), SEP)
            .await
            .unwrap()
            .unwrap()
    });
    {
        let is_match =
            matches!(&res, Multi{ans, others, ..} if *ans == pref.get_name() && others.len() == 2);
        assert!(is_match, "{:?} 並非預期中結果，應更新測資", res);
    }

    t2.can_find("multifuzz/t").unwrap();
    t1.run("").unwrap();
    t1.can_find("multifuzz/t").unwrap();
    t2.run("").unwrap();
    t2.can_find("multifuzz/t").unwrap();

    pref.can_find("multifuzz").unwrap();
}

#[test]
fn test_history_rm_range() {
    println!("移除一整段事件");
    let _g = setup();

    let t2 = ScriptTest::new("test2", None, None);
    t2.run("sep").unwrap();
    t2.run("a").unwrap();
    t2.run("b").unwrap();

    let t1 = ScriptTest::new("test", None, None);
    t1.run("sep").unwrap();
    t1.run("a").unwrap();
    t1.run("b").unwrap();
    t1.run("sep").unwrap();
    t2.run("b").unwrap(); // t2 而非 t1
    t1.run("a").unwrap();
    t1.run("b").unwrap();

    fn show_history(script: &ScriptTest) -> Vec<String> {
        let s = run!("history show {}", script.get_name()).unwrap();
        s.split('\n').map(|s| s.to_owned()).collect()
    }

    t1.can_find("-").expect("用最近期詢問找不到？");
    assert_eq!(show_history(&t1), vec!["b", "a", "sep"]);

    run!("history rm {} -- 1..3", t1.get_name()).unwrap(); // rm b & a
    assert_eq!(show_history(&t1), vec!["sep"]);

    t2.can_find("-").expect("刪除整段事件未能影響近期詢問");

    assert_eq!(
        show_history(&t2),
        vec!["b", "a", "sep"],
        "另一個腳本的歷史爛掉了"
    );
}

#[test]
fn test_fuzz_dot_or_endwith_slash() {
    println!("測試以`/`結尾的腳本名，以及`.`腳本名");
    let _g = setup();

    let t = ScriptTest::new("test/slash", None, None);
    t.can_find("test/").unwrap();
    t.can_find("t/").unwrap();
    t.can_find("slash/").unwrap();

    t.allow_other_error()
        .can_find("sla//")
        .expect_err("兩個`/`結尾仍不可行");
    t.allow_other_error().can_find("/sla").unwrap_err();

    run!("e illegal/").expect_err("不應創建以`/`結尾的腳本");

    run!("e .").expect_err("不應創建名為`.`的腳本");
    let t = ScriptTest::new(".1", None, None);
    run!("e .").expect("應查詢到剛才建出的腳本");
    t.can_find("1").unwrap();
    t.can_find(".1").unwrap();
    t.can_find(".").unwrap();

    t.allow_other_error()
        .can_find("..")
        .expect_err("兩個`.`仍不可行");
    t.allow_other_error().can_find(".a").unwrap_err();

    run!(allow_other_error: true, "mv - .").expect_err("不應創建名為`.`的腳本");
}

#[test]
fn test_existing_path() {
    println!("測試路徑衝突的邊角案例");
    let _g = setup();

    let _ = ScriptTest::new("dir/file", None, None);
    run!("e =dir -T txt | echo 1").expect_err("與目錄撞路徑");
    run!("e =dir/file.sh -T txt | echo 1").expect_err("與既存腳本撞路徑");
    run!("e =dir/file.sh/file | echo 1").unwrap_err();

    assert_ls_len(1, Some("all"), None);
}

#[test]
fn test_unknown_type_strange_ext() {
    println!("測試未知腳本類型");
    let _g = setup();

    let ty: ScriptType = "rb".parse().unwrap();
    let name = "this-name";
    let mut conf = load_conf();
    let ty_conf = conf.types.get_mut(&ty).unwrap();
    ty_conf.ext = Some("strange-ext".to_owned());
    conf.store().unwrap();

    run!("e {} -T rb | puts 1", name).unwrap();

    let which = run!("which -").unwrap();
    assert!(which.ends_with(&format!("/{}.strange-ext", name)));
    assert_eq!(run!("-").unwrap(), "1");

    let mut conf = load_conf();
    conf.types.remove(&ty);
    conf.store().unwrap();

    let which = run!("which -").unwrap();
    assert!(which.ends_with(&format!("/{}.rb", name)));
    run!("-").unwrap_err();

    assert_ls_len(1, None, None);
    run!("rm {}", name).unwrap();
    assert_ls_len(0, None, None);
    assert_ls_len(1, Some("all"), None);

    run!("mv -t all {}!", name).unwrap();
    assert_ls_len(1, None, None);

    run!("rm {} --purge", name).unwrap();
    assert_ls_len(0, None, None);
    assert_ls_len(0, Some("all"), None);
}

#[test]
fn test_multi_edit_conflict() {
    const TEST: &str = "this is a multi-edit test";
    let _g = setup();

    run!("e .1 ? | echo {}", TEST).unwrap();
    assert_ls_len(2, Some("all"), None);
    assert_eq!(TEST, run!(".1").unwrap());
    assert_eq!(TEST, run!(".2").unwrap());

    run!("e a b a .2 | echo {}", TEST).unwrap();
    assert_ls_len(4, Some("all"), None);
    assert_eq!(TEST, run!("a").unwrap());
    assert_eq!(TEST, run!("b").unwrap());
    assert_eq!(TEST, run!(".1").unwrap());
    assert_eq!(format!("{}\n{}", TEST, TEST), run!(".2").unwrap());
}

#[test]
fn test_first_command() {
    println!("第一次執行總是比較少見的案例，其它測項沒碰到它，就在這裡測吧！");

    fn run_first_cmd(cmd: &str) -> String {
        let _g = clean_and_set_home();
        run!("{}", cmd).unwrap()
    }
    fn assert_first_cmd(cmd: &str) {
        let first = run_first_cmd(cmd);
        run!("ls").unwrap(); // NOTE: call something that's very well tested
        let second = run!("{}", cmd).unwrap();
        assert_eq!(first, second);
    }

    assert_first_cmd("ls");
    // assert_first_cmd("types"); FIXME

    run_first_cmd("t gg");
    assert_eq!("", run!("ls").unwrap());
}

#[test]
fn test_alias_args() {
    println!("`hs script 'a b c'` & `hs script-alias 'a b c'` should be the same");

    let _g = setup();
    run!("e -T rb test | puts ARGV.inspect").unwrap();
    run!("alias test-alias test").unwrap();
    run!("alias test-alias-shell !ruby $HS_HOME/test.rb").unwrap();

    let expected = r#"["a b c"]"#;
    let normal_res = run!("test 'a b c'").unwrap();
    let alias_res = run!("test-alias 'a b c'").unwrap();
    let alias_shell_res = run!("test-alias-shell 'a b c'").unwrap();

    assert_eq!(expected, normal_res);
    assert_eq!(expected, alias_res);
    assert_eq!(expected, alias_shell_res);
}
