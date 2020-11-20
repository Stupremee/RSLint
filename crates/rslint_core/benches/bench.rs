use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use rslint_core::CstRuleStore;
use rslint_lexer::Lexer;
use rslint_parser::parse_text;
use rslint_scope::ScopeAnalyzer;

const ENGINE_262_URL: &str = "https://engine262.js.org/engine262/engine262.js";

fn parse(source: &str) {
    parse_text(source, 0);
}

fn tokenize(source: &str) {
    Lexer::from_str(source, 0).for_each(drop);
}

fn lint(analyzer: ScopeAnalyzer, source: &str) {
    let _ = rslint_core::lint_file(
        0,
        source,
        false,
        &CstRuleStore::new().builtins(),
        false,
        analyzer,
    );
}

fn bench_source(c: &mut Criterion, name: &str, source: &str) {
    let analyzer = ScopeAnalyzer::new().unwrap();

    let mut group = c.benchmark_group(name);
    group
        .sample_size(10)
        .throughput(Throughput::Bytes(source.len() as u64))
        .bench_function("tokenize", |b| b.iter(|| tokenize(black_box(&source))))
        .bench_function("parse", |b| b.iter(|| parse(black_box(&source))))
        .bench_function("lint", |b| {
            b.iter(|| lint(analyzer.clone(), black_box(&source)))
        });

    group.finish();
}

fn engine262(c: &mut Criterion) {
    let source = ureq::get(ENGINE_262_URL)
        .call()
        .into_string()
        .expect("failed to get engine262 source code");
    bench_source(c, "engine262", &source);
}

criterion_group!(benches, engine262);
criterion_main!(benches);
