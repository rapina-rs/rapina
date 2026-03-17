use rapina::prelude::*;
use serde::Deserialize;

// Single path parameter
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("user={}", *id)
}

// Two path parameters as a tuple
#[get("/orgs/:org_id/repos/:repo_id")]
async fn get_repo(Path((org_id, repo_id)): Path<(u64, u64)>) -> String {
    format!("org={} repo={}", org_id, repo_id)
}

// Three path parameters as a tuple
#[get("/orgs/:org_id/repos/:repo_id/issues/:issue_id")]
async fn get_issue(Path((org_id, repo_id, issue_id)): Path<(u64, u64, u64)>) -> String {
    format!("org={} repo={} issue={}", org_id, repo_id, issue_id)
}

// Mixed types in a tuple
#[get("/projects/:project_id/tasks/:task_name")]
async fn get_task(Path((project_id, task_name)): Path<(u32, String)>) -> String {
    format!("project={} task={}", project_id, task_name)
}

// Named struct — params matched by field name
#[derive(Deserialize)]
struct MemberParams {
    org_id: u64,
    team_id: u64,
    member_id: u64,
}

#[get("/orgs/:org_id/teams/:team_id/members/:member_id")]
async fn get_member(Path(p): Path<MemberParams>) -> String {
    format!("org={} team={} member={}", p.org_id, p.team_id, p.member_id)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new().discover().listen("127.0.0.1:3000").await
}
