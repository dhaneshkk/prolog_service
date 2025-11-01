use warp::Filter;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scryer_prolog::{
    MachineBuilder,
    StreamConfig,
    LeafAnswer,
    Term, // re-exported publicly — correct type for terms
};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct QueryRequest {
    program: String,
    query: String,
}

#[derive(Debug, Serialize)]
struct QueryResponse {
    results: Vec<serde_json::Value>,
}

#[tokio::main]
async fn main() {
    let route = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(handle_query);

    println!("🚀 Prolog service running at http://127.0.0.1:3030/query");
    warp::serve(route).run(([127, 0, 0, 1], 3030)).await;
}

async fn handle_query(req: QueryRequest) -> Result<impl warp::Reply, warp::Rejection> {
    match run_query(&req.program.as_str()  , &req.query.as_str()) {
        Ok(results) => Ok(warp::reply::json(&QueryResponse { results })),
        Err(err_msg) => {
            let err = json!({ "error": err_msg });
            Ok(warp::reply::json(&err))
        }
    }
}

fn run_query(program: &str, query: &str) -> Result<Vec<serde_json::Value>, String> {
    // Initialize in-memory I/O for Prolog
    let streams = StreamConfig::in_memory();

    let mut machine = MachineBuilder::new()
        .with_streams(streams)
        .build();

    // Load the program
    machine.consult_module_string("user", program);

    // Run the query
    let query_iter = machine.run_query(query);

    let mut results = Vec::new();

    for answer in query_iter {
        match answer {
            Ok(LeafAnswer::True) => {
                results.push(json!({ "result": true }));
            }
            Ok(LeafAnswer::False) => {
                results.push(json!({ "result": false }));
            }
            Ok(LeafAnswer::Exception(term)) => {
                results.push(json!({ "exception": format!("{:?}", term) }));
            }
            Ok(LeafAnswer::LeafAnswer { bindings, .. }) => {
                results.push(convert_bindings_to_json(bindings));
            }
            Err(e) => {
                results.push(json!({ "error": format!("{:?}", e) }));
            }
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
