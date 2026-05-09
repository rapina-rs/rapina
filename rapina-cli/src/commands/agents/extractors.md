## Extractors (handler argument order matters)

```
state: State<T>           // always first
id: Path<i32>             // URL path param (:id syntax)
params: Query<T>          // query string
user: CurrentUser         // authenticated user (id, claims)
ctx: Context              // request context (trace_id, start_time)
db: Db                    // database connection (requires database feature)
jar: Cookie<T>            // cookie values
body: Json<T>             // JSON body — only one body extractor per handler
body: Validated<Json<T>>  // JSON body with validation, returns 422 on failure
body: Form<T>             // form data
```
