# 🧠 Prolog Service — JSON-over-HTTP with Scryer Prolog + Rust (Warp)

A lightweight, **multi-core Prolog engine microservice** built with Rust’s [`warp`](https://crates.io/crates/warp) framework and the [`scryer-prolog`](https://crates.io/crates/scryer-prolog) engine.  
It allows you to send Prolog programs and queries via **HTTP POST** and receive structured **JSON results**.


---

## 🚀 Features

- ⚙️ **Scryer Prolog embedded** — executes real Prolog code inside Rust.
- 🌐 **RESTful API** — communicate using plain JSON.
- 🧵 **Multi-core runtime** — uses all available CPU cores (Tokio multi-thread scheduler).
- 🧩 **Safe concurrent queries** — isolates each query via `spawn_blocking`.
- 📦 **Easy to deploy** — simple `cargo run` or Docker container.

---

## 🧰 Requirements

- **Rust** (v1.70 or newer)
- **Cargo**
- Optional: **Docker**

---

## 🏗️ Build & Run

### 1️⃣ Clone and build

```bash
git clone https://github.com/yourname/prolog_service.git
cd prolog_service
cargo build --release

```bash
curl -X POST http://localhost:3030/query \
  -H "Content-Type: application/json" \
  -d '{"program": "parent(john, mary). parent(mary, alice). ancestor(X,Y) :- parent(X,Y). ancestor(X,Y) :- parent(X,Z), ancestor(Z,Y).", "query": "ancestor(john, Who)."}'
{"results":[{"Who":"Atom(\"mary\")"},{"Who":"Atom(\"alice\")"},{"result":false}]
```

