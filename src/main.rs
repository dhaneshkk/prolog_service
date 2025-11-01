use warp::Filter;
use serde::{Deserialize, Serialize};
use serde_json::json;
use scryer_prolog::{MachineBuilder, StreamConfig, LeafAnswer, Term};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Semaphore;

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
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    println!("🚀 Prolog multi-core service starting with {num_threads} threads...");
    println!("🌍 Listening on http://0.0.0.0:3030/query");
    println!("💚 Health check: http://0.0.0.0:3030/health");

    let semaphore = Arc::new(Semaphore::new(num_threads));

    let query_route = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_semaphore(semaphore.clone()))
        .and_then(handle_query);

    let health_route = warp::path("health")
        .and(warp::get())
        .map(|| warp::reply::json(&json!({ "status": "ok" })));

    let routes = query_route.or(health_route);

    let (_, server) = warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], 3030), async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for shutdown signal");
            println!("\n👋 Shutting down gracefully...");
        });

    server.await;
}

fn with_semaphore(
    sem: Arc<Semaphore>,
) -> impl Filter<Extract = (Arc<Semaphore>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || sem.clone())
}

async fn handle_query(
    req: QueryRequest,
    semaphore: Arc<Semaphore>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let _permit = semaphore.acquire_owned().await.unwrap();

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
        Ok(Err(err_msg)) => Ok(warp::reply::json(&json!({ "error": err_msg }))),
        Err(join_err) => Ok(warp::reply::json(&json!({
            "error": format!("Task join error: {join_err}")
        }))),
    }
}

fn run_query(program: &str, query: &str) -> Result<Vec<serde_json::Value>, String> {
    let streams = StreamConfig::in_memory();
    let mut machine = MachineBuilder::new().with_streams(streams).build();

    // In current scryer-prolog, this returns `()`
    machine.consult_module_string("user", program);

    let query_iter = machine.run_query(query);
    let mut results = Vec::new();

    for answer in query_iter {
        match answer {
            Ok(LeafAnswer::True) => results.push(json!({ "result": true })),
            Ok(LeafAnswer::False) => results.push(json!({ "result": false })),
            Ok(LeafAnswer::Exception(term)) => {
                results.push(json!({ "exception": term_to_json(&term) }))
            }
            Ok(LeafAnswer::LeafAnswer { bindings, .. }) => {
                results.push(convert_bindings_to_json(bindings))
            }
            Err(e) => results.push(json!({ "error": format!("{:?}", e) })),
        }
    }

    Ok(results)
}

fn convert_bindings_to_json(bindings: BTreeMap<String, Term>) -> serde_json::Value {
    let json_map: BTreeMap<String, serde_json::Value> = bindings
        .into_iter()
        .map(|(k, v)| (k, term_to_json(&v)))
        .collect();
    json!(json_map)
}

/// Convert `scryer_prolog::Term` into a JSON value (recursive and structured)
fn term_to_json(term: &Term) -> serde_json::Value {
    match term {
        Term::Integer(i) => json!(i.to_string()),
        Term::Rational(r) => json!(r.to_string()),
        Term::Float(f) => json!(f),
        Term::Atom(a) => json!(a),
        Term::String(s) => json!(s),
        Term::List(items) => {
            let json_items: Vec<serde_json::Value> =
                items.iter().map(term_to_json).collect();
            json!(json_items)
        }
        Term::Compound(name, args) => json!({
            "functor": name,
            "args": args.iter().map(term_to_json).collect::<Vec<_>>()
        }),
        Term::Var(v) => json!({ "var": v }),
        // Future-proofing: handle any new variants as strings
        _ => json!(format!("{:?}", term)),
    }
}
