//! Pagination example.
//!
//! Run with `cargo run --example pagination`
//!
//! Endpoints:
//! - GET /articles?page=1&per_page=2  — List articles with pagination

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
    ]
}

#[get("/articles")]
async fn list_articles(page: Paginate) -> Result<Json<Paginated<Article>>> {
    let all_articles = articles();
    let total = all_articles.len() as u64;

    let total_pages = total.div_ceil(page.per_page);
    let start = ((page.page - 1) * page.per_page) as usize;

    let data = all_articles
        .into_iter()
        .skip(start)
        .take(page.per_page as usize)
        .collect();

    Ok(Json(Paginated {
        data,
        page: page.page,
        per_page: page.per_page,
        total,
        total_pages,
        has_prev: page.page > 1,
        has_next: page.page < total_pages,
    }))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:3000";

    println!("Rapina Pagination Example");
    println!("  Usage:");
    println!("    GET /articles");
    println!("    GET /articles?page=2&per_page=1");

    Rapina::new()
        .state(PaginationConfig {
            default_per_page: 5,
            max_per_page: 20,
        })
        .discover()
        .listen(addr)
        .await
}
