use warp::Filter;
use serde::{Deserialize, Serialize};
use serde_json::json;
use scryer_prolog::{MachineBuilder, StreamConfig, LeafAnswer, Term};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Semaphore;
use dotenvy::dotenv;
use std::env;
use log::{info, warn, error};
use flexi_logger::{Duplicate, FileSpec, Logger, Cleanup, Criterion, Naming, Age, WriteMode, LoggerHandle};
use once_cell::sync::OnceCell;

use std::fs;
use std::path::Path;
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
    // 🧩 Load environment variables
    dotenv().ok();

    // Default port = 3030, can override via .env
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3030);

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // 🧾 Initialize rotating logger
    init_logger();

    info!("🚀 Prolog service starting with {num_threads} threads...");
    info!("🌍 Listening on http://0.0.0.0:{port}/query");

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

    let program: Arc<String> = Arc::new(req.program);
    let query: Arc<String> = Arc::new(req.query);

    info!("🧩 Handling query: {}", query);

    let res = tokio::task::spawn_blocking({
        let program = Arc::clone(&program);
        let query = Arc::clone(&query);
        move || run_query(&program, &query)
    })
        .await;

    match res {
        Ok(Ok(results)) => Ok(warp::reply::json(&QueryResponse { results })),
        Ok(Err(err_msg)) => {
            error!("❌ Query error: {}", err_msg);
            Ok(warp::reply::json(&json!({ "error": err_msg })))
        }
        Err(join_err) => {
            error!("❌ Task join error: {:?}", join_err);
            Ok(warp::reply::json(&json!({
                "error": format!("Task join error: {join_err}")
            })))
        }
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

fn term_to_json(term: &Term) -> serde_json::Value {
    match term {
        Term::Integer(i) => json!(i.to_string()),
        Term::Rational(r) => json!(r.to_string()),
        Term::Float(f) => json!(f),
        Term::Atom(a) => json!(a),
        Term::String(s) => json!(s),
        Term::List(items) => json!(items.iter().map(term_to_json).collect::<Vec<_>>()),
        Term::Compound(name, args) => json!({
            "functor": name,
            "args": args.iter().map(term_to_json).collect::<Vec<_>>()
        }),
        Term::Var(v) => json!({ "var": v }),
        _ => json!(format!("{:?}", term)),
    }
}

/// 🪶 Setup daily rotating logs
// Global static to keep logger alive
static LOGGER_HANDLE: OnceCell<LoggerHandle> = OnceCell::new();

pub fn init_logger() {
    let default_log_dir = "/var/log/prolog_service";
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| {
        if Path::new(default_log_dir).exists() {

            default_log_dir.to_string()
        } else {
            "./logs".to_string()
        }
    });

    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("⚠️ Failed to create log directory {log_dir}: {e}");
        eprintln!("👉 Falling back to stderr logging.");
        let handle = Logger::try_with_env_or_str("info")
            .unwrap()
            .duplicate_to_stderr(Duplicate::All)
            .write_mode(WriteMode::Direct)
            .start()
            .unwrap();
        LOGGER_HANDLE.set(handle).ok();
        return;
    }

    let file_spec = FileSpec::default()
        .directory(&log_dir)
        .basename("prolog_service")
        .suffix("log");

    let logger = Logger::try_with_env_or_str("info")
        .unwrap()
        .log_to_file(file_spec)
        .duplicate_to_stderr(Duplicate::Info)
        .write_mode(WriteMode::Async)
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(7),
        )
        .start();

    match logger {
        Ok(handle) => {
            LOGGER_HANDLE.set(handle).ok(); // Keep it alive globally
            info!("🪵 Logging initialized successfully in: {log_dir}");
        }
        Err(e) => {
            eprintln!("⚠️ Failed to initialize file logger: {e}");
            eprintln!("👉 Falling back to stderr logging.");
            let handle = Logger::try_with_env_or_str("info")
                .unwrap()
                .duplicate_to_stderr(Duplicate::All)
                .write_mode(WriteMode::Direct)
                .start()
                .unwrap();
            LOGGER_HANDLE.set(handle).ok();
        }
    }
}


