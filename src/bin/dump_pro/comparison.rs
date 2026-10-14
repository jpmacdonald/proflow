//! Compare two native presentation documents as JSON.

use super::load_presentation;
use std::path::Path;

pub fn diff_presentations(path1: &Path, path2: &Path) {
    let pres1 = load_presentation(path1);
    let pres2 = load_presentation(path2);

    let json1 =
        serde_json::to_string_pretty(&pres1).expect("failed to serialize presentation 1 as JSON");
    let json2 =
        serde_json::to_string_pretty(&pres2).expect("failed to serialize presentation 2 as JSON");

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║ Comparing:");
    println!(
        "║ 1: {}",
        path1
            .file_name()
            .expect("path1 has no filename")
            .to_string_lossy()
    );
    println!(
        "║ 2: {}",
        path2
            .file_name()
            .expect("path2 has no filename")
            .to_string_lossy()
    );
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    if json1 == json2 {
        println!("✅ Files are identical");
        return;
    }

    let lines1: Vec<&str> = json1.lines().collect();
    let lines2: Vec<&str> = json2.lines().collect();

    println!("📊 Size comparison:");
    println!("   File 1: {} lines, {} bytes", lines1.len(), json1.len());
    println!("   File 2: {} lines, {} bytes", lines2.len(), json2.len());
    println!();

    println!("🔍 First 50 differences:");
    let mut diff_count = 0;
    let max_len = lines1.len().max(lines2.len());

    for i in 0..max_len {
        let l1 = lines1.get(i).copied().unwrap_or("");
        let l2 = lines2.get(i).copied().unwrap_or("");

        if l1 != l2 {
            diff_count += 1;
            if diff_count <= 50 {
                println!("Line {}:", i + 1);
                if !l1.is_empty() {
                    println!("  - {}", l1.trim());
                }
                if !l2.is_empty() {
                    println!("  + {}", l2.trim());
                }
            }
        }
    }

    println!();
    println!("📈 Total differences: {diff_count} lines");
}
