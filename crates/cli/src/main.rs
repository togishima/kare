use clap::Parser;

/// Test suite health check from CI artifacts — built for PHPUnit,
/// accepts any JUnit XML.
#[derive(Parser)]
#[command(name = "kare", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!("kare: no analysis implemented yet — commands land in upcoming milestones");
}
