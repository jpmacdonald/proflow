use proflow::propresenter::generated::rv_data;
use prost::Message;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <presentation.pro>", args[0]);
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    let bytes = fs::read(&path)?;
    let presentation = rv_data::Presentation::decode(&bytes[..])?;

    // Dump the entire presentation structure
    println!("{:#?}", presentation);

    Ok(())
} 