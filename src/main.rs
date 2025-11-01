use warp::Filter;
use serde::{Deserialize, Serialize};
use serde_json::json;
use scryer_prolog::{MachineBuilder, StreamConfig, LeafAnswer, Term};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct QueryRequest {
    program: String,
    query: String,
}

#[derive(Debug, Serialize)]
struct QueryResponse {
    results: Vec<serde_json::Value>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // Tokio’s multi-thread flavor automatically uses all available CPU cores.
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    println!("🚀 Prolog multi-core service starting with {num_threads} threads...");
    println!("🌍 Listening on http://0.0.0.0:3030/query");

    let route = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(handle_query);

    warp::serve(route)
        .run(([0, 0, 0, 0], 3030))
        .await;
}

async fn handle_query(req: QueryRequest) -> Result<impl warp::Reply, warp::Rejection> {
    // Explicit Arc type to ensure we’re using std::sync::Arc
    let program: Arc<String> = Arc::new(req.program);
    let query: Arc<String> = Arc::new(req.query);

    let res = tokio::task::spawn_blocking({
        let program = Arc::clone(&program);
        let query = Arc::clone(&query);
        move || run_query(&program, &query)
    })
        .await;

    match res {
        Ok(Ok(results)) => Ok(warp::reply::json(&QueryResponse { results })),
        Ok(Err(err_msg)) => Ok(warp::reply::json(&json!({ "error": err_msg })) ),
        Err(join_err) => Ok(warp::reply::json(&json!({ "error": format!("Task join error: {join_err}") })) ),
    }
}


fn run_query(program: &str, query: &str) -> Result<Vec<serde_json::Value>, String> {
    let streams = StreamConfig::in_memory();
    let mut machine = MachineBuilder::new().with_streams(streams).build();

    machine.consult_module_string("user", program);
    let query_iter = machine.run_query(query);

    let mut results = Vec::new();
    for answer in query_iter {
        match answer {
            Ok(LeafAnswer::True) => results.push(json!({ "result": true })),
            Ok(LeafAnswer::False) => results.push(json!({ "result": false })),
            Ok(LeafAnswer::Exception(term)) => results.push(json!({ "exception": format!("{:?}", term) })),
            Ok(LeafAnswer::LeafAnswer { bindings, .. }) => {
                results.push(convert_bindings_to_json(bindings))
            }
            Err(e) => results.push(json!({ "error": format!("{:?}", e) })),
        }
    }
    Ok(results)
}

fn convert_bindings_to_json(bindings: BTreeMap<String, Term>) -> serde_json::Value {
    let json_map: BTreeMap<String, String> = bindings
        .into_iter()
        .map(|(k, v)| (k, format!("{:?}", v)))
        .collect();
    json!(json_map)
}
