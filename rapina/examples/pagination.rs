#![cfg(feature = "database")]

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
            title: "Getting started with Rapina".to_string(),
            author: "Ada".to_string(),
        },
        Article {
            id: 2,
            title: "Building JSON APIs".to_string(),
            author: "Grace".to_string(),
        },
        Article {
            id: 3,
            title: "Routing patterns".to_string(),
            author: "Linus".to_string(),
        },
        Article {
            id: 4,
            title: "Extractors in practice".to_string(),
            author: "Barbara".to_string(),
        },
        Article {
            id: 5,
            title: "Sharing state safely".to_string(),
            author: "Margaret".to_string(),
        },
        Article {
            id: 6,
            title: "Typed responses".to_string(),
            author: "Ken".to_string(),
        },
        Article {
            id: 7,
            title: "Pagination without a database".to_string(),
            author: "Radia".to_string(),
        },
        Article {
            id: 8,
            title: "OpenAPI from handlers".to_string(),
            author: "Alan".to_string(),
        },
        Article {
            id: 9,
            title: "Validation errors".to_string(),
            author: "Frances".to_string(),
        },
        Article {
            id: 10,
            title: "Deployment checklist".to_string(),
            author: "Donald".to_string(),
        },
    ]
}

#[get("/articles")]
async fn list_articles(page: Paginate) -> Paginated<Article> {
    let all_articles = articles();
    let total = all_articles.len() as u64;
    let total_pages = total.div_ceil(page.per_page);
    let start = ((page.page - 1) * page.per_page) as usize;
    let data = all_articles
        .into_iter()
        .skip(start)
        .take(page.per_page as usize)
        .collect();

    Paginated {
        data,
        page: page.page,
        per_page: page.per_page,
        total,
        total_pages,
        has_prev: page.page > 1,
        has_next: page.page < total_pages,
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new().get("/articles", list_articles);

    println!("Pagination example:");
    println!("  GET /articles");
    println!("  GET /articles?page=2&per_page=3");

    Rapina::new()
        .state(PaginationConfig {
            default_per_page: 5,
            max_per_page: 20,
        })
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
