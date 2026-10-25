#![forbid(unsafe_code)]
use xiaomu_perf_harness::{fixtures::SCENARIOS, runtime};
fn main() {
    if cfg!(debug_assertions) {
        eprintln!("performance measurements require a release build");
        std::process::exit(2);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--list") {
        println!("{}", SCENARIOS.join("\n"));
        return;
    }
    let scenario = args.first().map(String::as_str).unwrap_or("long-1k");
    let samples = args
        .get(1)
        .map(|s| s.parse().expect("samples"))
        .unwrap_or(30);
    let history = args
        .get(2)
        .map(|s| s.parse().expect("history edits"))
        .unwrap_or(256);
    println!("{}", runtime::run(scenario, samples, history));
}
