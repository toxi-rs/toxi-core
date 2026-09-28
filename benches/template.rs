//! Template benches: render a small page with a loop.

use divan::Bencher;
use toxi_template::{Context, TemplateEngine};

fn main() {
    divan::main();
}

fn engine() -> TemplateEngine {
    let mut engine = TemplateEngine::new();
    engine
        .add_template(
            "page",
            "<h1>{{ title }}</h1><ul>{% for item in items %}<li>{{ item }}</li>{% endfor %}</ul>",
        )
        .unwrap();
    engine
}

fn context() -> Context {
    let mut ctx = Context::new();
    ctx.set("title", "Hello");
    ctx.set("items", vec!["a", "b", "c", "d"]);
    ctx
}

#[divan::bench]
fn template_render(bencher: Bencher) {
    let engine = engine();
    bencher
        .with_inputs(context)
        .bench_values(|ctx| divan::black_box(engine.render("page", &ctx)));
}
