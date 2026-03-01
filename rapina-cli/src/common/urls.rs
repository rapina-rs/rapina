const PATH_BASE: &str = "__rapina";

pub fn build_routes_url(host: &str, port: u16) -> String {
    format!("http://{}:{}/{}/routes", host, port, PATH_BASE)
}

pub fn build_openapi_url(host: &str, port: u16) -> String {
    format!("http://{}:{}/{}/openapi.json", host, port, PATH_BASE)
}
