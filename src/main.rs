use clap::Parser;

/// kiss - a command-line tool
#[derive(Parser, Debug)]
#[command(name = "kiss", version, about)]
struct Args {
    /// Input to process
    input: Option<String>,
}

fn main() {
    let args = Args::parse();

    if let Some(input) = args.input {
        println!("Input: {input}");
    } else {
        println!("No input provided");
    }
}

