#![feature(custom_test_frameworks)]
#![test_runner(criterion::runner)]

use criterion::{black_box, Criterion};
use criterion_macro::criterion;
use hyper_scripter::fuzzy::*;
use rand::{rngs::StdRng, seq::index::sample, Rng, SeedableRng};
use std::borrow::Cow;

#[allow(dead_code)]
#[path = "../tests/tool.rs"]
mod tool;
use tool::*;

const LONG: usize = 20;
const SHORT: std::ops::Range<usize> = 5..15;
fn gen_name(rng: &mut StdRng) -> String {
    const CHARSET: &[u8] = b"///_ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                                    abcdefghijklmnopqrstuvwxyz\
                                    0123456789";
    loop {
        let s: String = (0..LONG)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();

        if s.starts_with("/") || s.ends_with("/") {
            continue;
        }
        if s.find("//").is_some() {
            continue;
        }
        return s;
    }
}
fn sample_name(rng: &mut StdRng, name: &str) -> String {
    let mut ret = "".to_owned();
    let len = rng.gen_range(SHORT);
    let mut idx_sample: Vec<_> = sample(rng, LONG, len).iter().collect();
    idx_sample.sort();
    for idx in idx_sample.into_iter() {
        ret.push(name.chars().nth(idx).unwrap());
    }
    ret
}

#[criterion]
fn bench_fuzz(c: &mut Criterion) {
    let _ = env_logger::try_init();

    struct MyStr<'a>(&'a str);
    impl<'a> FuzzKey for MyStr<'a> {
        fn fuzz_key(&self) -> Cow<'a, str> {
            Cow::Borrowed(self.0)
        }
    }

    let mut rng = StdRng::seed_from_u64(42);
    const CASE_COUNT: usize = 999;

    let mut names = vec![];
    let mut shorts = vec![];
    for _ in 0..CASE_COUNT {
        let name = gen_name(&mut rng);
        let short = sample_name(&mut rng, &name);
        names.push(name);
        shorts.push(short);
    }

    let mut rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("fuzzy_func", |b| {
        b.iter(|| {
            rt.block_on(async {
                for short in shorts.iter() {
                    let names = names.iter().map(|s| MyStr(s.as_ref()));
                    let res = fuzz(short, names).await.unwrap();
                    black_box(res);
                }
            });
        });
    });
}

struct TestDate {
    data: Vec<(String, [i8; 3])>,
}
impl TestDate {
    fn new(count: usize, rng: &mut StdRng) -> Self {
        let mut data = vec![];
        for _ in 0..count {
            let name = gen_name(rng);
            data.push((name, gen_tag_arr(rng, 0, 1)));
        }
        TestDate { data }
    }
    fn setup(&self) {
        let _ = setup();
        for (name, tag_arr) in self.data.iter() {
            let tag_str = gen_tag_string(tag_arr);
            run(format!("e -t {} {} | echo $NAME", tag_str, name)).unwrap();
        }
    }
}
fn gen_tag_arr(rng: &mut StdRng, min: i8, max: i8) -> [i8; 3] {
    let mut tags = [0; 3];
    for j in 0..3 {
        tags[j] = rng.gen_range(min..=max);
    }
    tags
}
fn gen_tag_string(a: &[i8; 3]) -> String {
    let mut v = vec![];
    for (i, &u) in a.iter().enumerate() {
        match u {
            1 => v.push(format!("tag{}", i)),
            -1 => v.push(format!("^tag{}", i)),
            _ => (),
        }
    }
    if v.is_empty() {
        "all".to_owned()
    } else {
        v.join(",")
    }
}
fn gen_tag_filter_string(rng: &mut StdRng, mut a: [i8; 3]) -> String {
    for i in 0..3 {
        let should_messup = rng.gen_bool(0.5);
        if should_messup {
            a[i] = rng.gen_range(-1..=1);
        }
    }
    gen_tag_string(&a)
}

const CASES: usize = 200;
const ITER_COUNT: usize = 400;
#[criterion]
fn bench_massive_fuzzy(c: &mut Criterion) {
    // 200 scripts, with random tags from [tag1, tag2, tag3] (2^3 posible combinations)
    // run with random tag, with random (80% right, 20% wrong) fuzzy name
    let mut rng = StdRng::seed_from_u64(42);
    let data = TestDate::new(CASES, &mut rng);
    let args: Vec<_> = (0..ITER_COUNT)
        .into_iter()
        .map(|i| {
            if i % 10 == 0 {
                let tag_num = (i / 10) % 3;
                return format!("tags +tag{}", tag_num);
            }
            let i = rng.gen_range(0..CASES);
            let name = sample_name(&mut rng, &data.data[i].0);
            let filter = gen_tag_filter_string(&mut rng, data.data[i].1.clone());
            format!("-f +{} {}", filter, name)
        })
        .collect();

    c.bench_function("massive_fuzzy", |b| {
        b.iter_with_setup(
            || data.setup(),
            |_| {
                for arg in args.iter() {
                    let _ = run(arg);
                }
            },
        );
    });
}

#[criterion]
fn bench_massive_exact(c: &mut Criterion) {
    // 200 scripts, with random tags from [tag1, tag2, tag3] (2^3 posible combinations)
    // run with random tag, with random (80% right, 20% wrong) exact name
    let mut rng = StdRng::seed_from_u64(42);
    let data = TestDate::new(CASES, &mut rng);
    let args: Vec<_> = (0..ITER_COUNT)
        .into_iter()
        .map(|i| {
            if i % 10 == 0 {
                let tag_num = (i / 10) % 3;
                return format!("tags +tag{}", tag_num);
            }
            let i = rng.gen_range(0..CASES);
            let name = &data.data[i].0;
            let filter = gen_tag_filter_string(&mut rng, data.data[i].1.clone());
            format!("-f +{} ={}", filter, name)
        })
        .collect();

    c.bench_function("massive_exact", |b| {
        b.iter_with_setup(
            || data.setup(),
            |_| {
                for arg in args.iter() {
                    let _ = run(arg);
                }
            },
        );
    });
}

#[criterion]
fn bench_massive_prev(c: &mut Criterion) {
    // 200 scripts, with random tags from [tag1, tag2, tag3] (2^3 posible combinations)
    // run with random tag, with random (^1 ~ ^200) previous query
    let mut rng = StdRng::seed_from_u64(42);
    let data = TestDate::new(CASES, &mut rng);
    let args: Vec<_> = (0..ITER_COUNT)
        .into_iter()
        .map(|i| {
            if i % 10 == 0 {
                let tag_num = (i / 10) % 3;
                return format!("tags +tag{}", tag_num);
            }
            let tag_str = gen_tag_string(&gen_tag_arr(&mut rng, -1, 1));
            let prev = rng.gen_range(1..=CASES);
            format!("-f +{} ^{}", tag_str, prev)
        })
        .collect();

    c.bench_function("massive_prev", |b| {
        b.iter_with_setup(
            || data.setup(),
            |_| {
                for arg in args.iter() {
                    let _ = run(arg);
                }
            },
        );
    });
}
