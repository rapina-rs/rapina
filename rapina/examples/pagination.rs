//! Pagination example.
//!
//! Run with `cargo run --example pagination --features database`
//!
//!
//! One endpoint, driven entirely by query params. Try:
//!   GET /articles              → default page size (5, from PaginationConfig)
//!   GET /articles?page=2       → page 2, still size 5 → returns the 6th article on the 2nd page
//!   GET /articles?per_page=2   → explicit override, 2 per page
//!   GET /articles?per_page=21  → 422: exceeds max_per_page (20)

use rapina::pagination::{Paginate, Paginated, PaginationConfig};
use rapina::prelude::*;

#[derive(Clone, Serialize, JsonSchema)]
struct Article {
    id: u64,
    title: String,
    author: String,
}

fn articles() -> Vec<Article> {
    vec![
        Article {
            id: 1,
            title: "Pagination example for Rapina".to_string(),
            author: "Crazy Diamond".to_string(),
        },
        Article {
            id: 2,
            title: "The AI tragedy of 2026".to_string(),
            author: "Killer Queen".to_string(),
        },
        Article {
            id: 3,
            title: "How to make your own framework in Rust".to_string(),
            author: "JoJo".to_string(),
        },
        Article {
            id: 4,
            title: "Why compilation errors are actually a love language".to_string(),
            author: "Star Platinum".to_string(),
        },
        Article {
            id: 5,
            title: "Overcoming the urge to rewrite everything in Rust.".to_string(),
            author: "Gold Experience".to_string(),
        },
        Article {
            id: 6,
            title: "How I accidentally deleted production using a smart pointer".to_string(),
            author: "The Hand".to_string(),
        },
    ]
}

#[get("/articles")]
async fn list_articles(page: Paginate) -> Result<Paginated<Article>> {
    let all_articles = articles();
    let total = all_articles.len() as u64;

    let total_pages = total.div_ceil(page.per_page);
    let start = ((page.page - 1) * page.per_page) as usize;

    let data = all_articles
        .into_iter()
        .skip(start)
        .take(page.per_page as usize)
        .collect();

    Ok(Paginated {
        data,
        page: page.page,
        per_page: page.per_page,
        total,
        total_pages,
        has_prev: page.page > 1,
        has_next: page.page < total_pages,
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:9999";

    println!("Rapina Pagination Example");
    println!("  Try:");
    println!("    GET /articles              # default page size (5)");
    println!("    GET /articles?page=2       # page 2 → the 6th article");
    println!("    GET /articles?per_page=2   # explicit override");
    println!("    GET /articles?per_page=21  # 422: exceeds max (20)");

    Rapina::new()
        .state(PaginationConfig {
            default_per_page: 5,
            max_per_page: 20,
        })
        .discover()
        .listen(addr)
        .await
}
