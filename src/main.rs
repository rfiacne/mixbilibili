mod cli;
mod ffmpeg;
mod scanner;
mod merger;

use clap::Parser;
use cli::Args;

fn main() {
    let mut args = Args::parse();

    if let Err(e) = args.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Validate format early
    let format = match args.parsed_format() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("Source: {:?}", args.source);
    println!("Output: {:?}", args.output);
    println!("Format: {}", format);
    println!("Jobs: {}", args.jobs);
    println!("Delete source: {}", args.sdel);
}