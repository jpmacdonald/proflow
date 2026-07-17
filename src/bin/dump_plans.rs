//! Dump upcoming plan items from Planning Center for analysis.
//!
//! Usage: `cargo run --bin dump_plans [-- --days 60] [--past]`

#![allow(clippy::uninlined_format_args)]

use proflow::config::Config;
use proflow::planning_center::api::PlanningCenterClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let days: i64 = args
        .iter()
        .position(|a| a == "--days")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let past = args.iter().any(|arg| arg == "--past");

    let config = Config::load()?;
    let client = PlanningCenterClient::new(&config)?;

    let (services, plans) = if past {
        client.get_recent_services(days).await?
    } else {
        client.get_upcoming_services(days).await?
    };

    println!("=== Services ({}) ===", services.len());
    for s in &services {
        println!("  {} (id: {})", s.name, s.id);
    }

    let scope = if past { "Recent Plans" } else { "Plans" };
    println!("\n=== {scope} ({}) ===\n", plans.len());
    for plan in &plans {
        println!(
            "--- {} | {} | {} | plan id: {} ---",
            plan.service_name,
            plan.title,
            plan.date.format("%Y-%m-%d"),
            plan.id
        );

        let items = client
            .get_service_items(&plan.id)
            .await
            .unwrap_or_else(|e| {
                eprintln!("  Error fetching items: {e}");
                vec![]
            });

        for item in &items {
            let cat = format!("{:?}", item.category);
            let song_info = item
                .song
                .as_ref()
                .map(|s| {
                    format!(
                        " [song: \"{}\", author: {:?}, arr: {:?}]",
                        s.title,
                        s.author.as_deref().unwrap_or("-"),
                        s.arrangement.as_deref().unwrap_or("-"),
                    )
                })
                .unwrap_or_default();
            let scripture_info = item
                .scripture
                .as_ref()
                .map(|s| format!(" [scripture: \"{}\"]", s.reference))
                .unwrap_or_default();
            let note = item.note.as_deref().unwrap_or("");
            let note_info = if note.is_empty() {
                String::new()
            } else {
                format!("\n      note: {}", note.replace('\n', "\n            "))
            };
            let desc_info = item
                .description
                .as_deref()
                .filter(|d| !d.is_empty())
                .map(|d| format!("\n      desc: {}", d.replace('\n', "\n            ")))
                .unwrap_or_default();

            println!(
                "  {:>2}. [{:<8}] {}{}{}{}{}",
                item.position, cat, item.title, song_info, scripture_info, note_info, desc_info
            );
        }
        println!();
    }

    Ok(())
}
