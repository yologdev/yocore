//! Test memory ranking against dev database
//!
//! Run with: cargo run --example test_ranking

use yocore::ai::ranking::{get_ranking_stats, rank_project_memories};
use yocore::db::Database;

fn main() {
    // Use dev database
    let db_path = dirs::home_dir()
        .unwrap()
        .join("Library/Application Support/com.yolog.desktop.dev/yolog.db");

    println!("Using database: {}", db_path.display());

    let db = Database::new(db_path).expect("Failed to open database");

    // Get project ID
    let project_id: String = {
        let conn = db.conn();
        conn.query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
            .expect("No projects found")
    };

    println!("Testing project: {}\n", project_id);

    // Get stats before
    println!("=== Before Ranking ===");
    let stats = get_ranking_stats(&db, &project_id).unwrap();
    println!("{}\n", serde_json::to_string_pretty(&stats).unwrap());

    // Run ranking (use larger batch to process all memories)
    let batch_size = 2000;
    println!("=== Running Ranking (batch={}) ===", batch_size);
    let result = rank_project_memories(&db, &project_id, batch_size).unwrap();
    println!("Evaluated: {}", result.memories_evaluated);
    println!("Promoted:  {}", result.promoted);
    println!("Demoted:   {}", result.demoted);
    println!("Removed:   {}", result.removed);
    println!("Unchanged: {}", result.unchanged);

    if !result.transitions.is_empty() {
        println!("\nFirst 10 Transitions:");
        for t in result.transitions.iter().take(10) {
            println!(
                "  [{}] {} -> {} (score: {:.3}) - {}",
                t.memory_id, t.from_state, t.to_state, t.score, t.reason
            );
        }
        if result.transitions.len() > 10 {
            println!("  ... and {} more", result.transitions.len() - 10);
        }
    }

    // Get stats after
    println!("\n=== After Ranking ===");
    let stats = get_ranking_stats(&db, &project_id).unwrap();
    println!("{}", serde_json::to_string_pretty(&stats).unwrap());
}
