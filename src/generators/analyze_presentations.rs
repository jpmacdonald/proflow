use proflow::propresenter::generated::rv_data;
use prost::Message;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

fn dump_presentation(path: &PathBuf) -> Result<rv_data::Presentation, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    Ok(rv_data::Presentation::decode(&bytes[..])?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create analysis directory if it doesn't exist
    fs::create_dir_all("analysis")?;

    // First dump the original presentation
    let original = dump_presentation(&PathBuf::from("data/examples/propresenter/[Hymn] Amazing Grace.pro"))?;
    let mut original_file = File::create("analysis/original.txt")?;
    writeln!(original_file, "{:#?}", original)?;

    // Then dump our generated presentation
    let generated = dump_presentation(&PathBuf::from("amazing_grace_recreated.pro"))?;
    let mut generated_file = File::create("analysis/generated.txt")?;
    writeln!(generated_file, "{:#?}", generated)?;

    // Create a diff using the run_terminal_cmd
    std::process::Command::new("diff")
        .arg("-u")
        .arg("analysis/original.txt")
        .arg("analysis/generated.txt")
        .output()
        .and_then(|output| {
            let mut diff_file = File::create("analysis/diff.txt")?;
            diff_file.write_all(&output.stdout)?;
            Ok(())
        })?;

    println!("Analysis complete! Check the analysis directory for the results.");
    Ok(())
} 