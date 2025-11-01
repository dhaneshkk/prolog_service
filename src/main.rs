use warp::Filter;
use serde::{Deserialize, Serialize};
use serde_json::json;
use scryer_prolog::{MachineBuilder, StreamConfig, LeafAnswer, Term};
use std::{collections::BTreeMap, env, sync::Arc};
use tokio::sync::Semaphore;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

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
    // --- 🌿 Load .env and initialize logging ---
    dotenvy::dotenv().ok();

    let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level.parse::<Level>().unwrap_or(Level::INFO))
        .with_target(false)
        .with_thread_names(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Setting default tracing subscriber failed");

    // --- ⚙️ Configuration ---
    let port: u16 = env::var("SERVER_PORT")
        .unwrap_or_else(|_| "3030".to_string())
        .parse()
        .unwrap_or(3030);

    let max_jobs: usize = env::var("MAX_CONCURRENT_JOBS")
        .unwrap_or_else(|_| "4".to_string())
        .parse()
        .unwrap_or(4);

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    info!("🚀 Starting Prolog service");
    info!("🧠 Available CPU threads: {num_threads}");
    info!("⚙️  Max concurrent Prolog jobs: {max_jobs}");
    info!("🌍 Listening on http://0.0.0.0:{port}");
    info!("💚 Health check endpoint: http://0.0.0.0:{port}/health");

    let semaphore = Arc::new(Semaphore::new(max_jobs));

    // --- 🛣️ Routes ---
    let query_route = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_semaphore(semaphore.clone()))
        .and_then(handle_query);

    let health_route = warp::path("health")
        .and(warp::get())
        .map(|| warp::reply::json(&json!({ "status": "ok" })));

    let routes = query_route.or(health_route);

    // --- 🌙 Graceful shutdown ---
    let (_, server) = warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for shutdown signal");
            warn!("🛑 Received termination signal. Shutting down gracefully...");
        });

    server.await;
    info!("👋 Server stopped cleanly.");

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
    info!("🧩 Received query: {}", req.query);

    let program: Arc<String> = Arc::new(req.program);
    let query: Arc<String> = Arc::new(req.query);

    let res = tokio::task::spawn_blocking({
        let program = Arc::clone(&program);
        let query = Arc::clone(&query);
        move || run_query(&program, &query)
    })
        .await;

    match res {
        Ok(Ok(results)) => {
            info!("✅ Query executed successfully with {} result(s)", results.len());
            Ok(warp::reply::json(&QueryResponse { results }))
        }
        Ok(Err(err_msg)) => {
            warn!("⚠️ Query execution failed: {}", err_msg);
            Ok(warp::reply::json(&json!({ "error": err_msg })))
        }
        Err(join_err) => {
            error!("💥 Task join error: {}", join_err);
            Ok(warp::reply::json(&json!({ "error": format!("Task join error: {join_err}") })))
        }
    }
}

fn run_query(program: &str, query: &str) -> Result<Vec<serde_json::Value>, String> {
    let streams = StreamConfig::in_memory();
    let mut machine = MachineBuilder::new().with_streams(streams).build();

    // Consult program (no longer returns Result)
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
            Err(e) => {
                let err_str = format!("{:?}", e);
                error!("Prolog execution error: {}", err_str);
                results.push(json!({ "error": err_str }));
            }
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

/// Recursive and structured converter for Prolog terms → JSON.
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
        _ => json!(format!("{:?}", term)),
    }
}
