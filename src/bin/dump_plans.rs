//! Dump upcoming plan items from Planning Center for analysis.
//!
//! Usage: cargo run --bin dump_plans [-- --days 60]

use proflow::config::Config;
use proflow::planning_center::api::PlanningCenterClient;

#[tokio::main]
async fn main() {
    let days: i64 = std::env::args()
        .position(|a| a == "--days")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let config = Config::load().expect("Failed to load config");
    let client = PlanningCenterClient::new(&config);

    let (services, plans) = client
        .get_upcoming_services(days)
        .await
        .expect("Failed to fetch services");

    println!("=== Services ({}) ===", services.len());
    for s in &services {
        println!("  {} (id: {})", s.name, s.id);
    }

    println!("\n=== Plans ({}) ===\n", plans.len());
    for plan in &plans {
        println!("--- {} | {} | {} ---", plan.service_name, plan.title, plan.date.format("%Y-%m-%d"));

        let items = client
            .get_service_items(&plan.id)
            .await
            .unwrap_or_else(|e| {
                eprintln!("  Error fetching items: {e}");
                vec![]
            });

        for item in &items {
            let cat = format!("{:?}", item.category);
            let song_info = item.song.as_ref().map(|s| {
                format!(
                    " [song: \"{}\", author: {:?}, arr: {:?}]",
                    s.title,
                    s.author.as_deref().unwrap_or("-"),
                    s.arrangement.as_deref().unwrap_or("-"),
                )
            }).unwrap_or_default();
            let scripture_info = item.scripture.as_ref().map(|s| {
                format!(" [scripture: \"{}\"]", s.reference)
            }).unwrap_or_default();
            let note = item.note.as_deref().unwrap_or("");
            let note_info = if note.is_empty() { String::new() } else { format!(" (note: {note})") };

            println!(
                "  {:>2}. [{:<8}] {}{}{}{}",
                item.position, cat, item.title, song_info, scripture_info, note_info
            );
        }
        println!();
    }
}
