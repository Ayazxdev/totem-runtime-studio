# Totem Runtime Studio (Sentinel Runtime)

**A secure, local-first, Rust-based execution runtime for autonomous AI agent workflows.**

Totem Runtime Studio is **not** another workflow builder. It is the **secure execution kernel** that runs underneath orchestration tools — comparable to how an OS kernel manages processes while higher-level tools manage them. Totem executes agent workflows **securely, observably, and reliably**.

---

## Positioning

| **Workflow Builders**           | **Totem Runtime Studio**       |
| ------------------------------- | ------------------------------ |
| Design and compose workflows    | **Execute workflows securely** |
| LangGraph, CrewAI, n8n, Flowise | **Sentinel Runtime**           |
| "What should the agent do?"     | "How do we safely run it?"     |

The runtime addresses problems that workflow builders cannot: **runtime security, deterministic execution, context explosion prevention, intelligent scheduling, tool verification, auditability, rollback/recovery, and production reliability.**

---

## Quick Start

### Prerequisites

- Rust 1.97.1+ — [install via rustup](https://rustup.rs/)

### Build & Run

```bash
# Terminal 1 — MCP test server (port 3031)
cd d:\A2A\totem-runtime-studio
cargo run -p mcp-test-server --quiet

# Terminal 2 — Sentinel runtime (port 3030)
cargo run --quiet -- --bind 127.0.0.1:3030
```

### Execute Your First Task

```bash
curl -X POST http://127.0.0.1:3030/execute \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello-totem",
    "actions": [{"tool_name": "echo", "parameters": {"message": "Hello from Totem Runtime!"}}]
  }'
```

---

## API Endpoints

| Method | Path                      | Description                       |
| ------ | ------------------------- | --------------------------------- |
| `POST` | `/execute`                | Execute a task workflow           |
| `GET`  | `/audit`                  | Retrieve hash-chained audit trail |
| `GET`  | `/health`                 | Health check + version            |
| `GET`  | `/tools`                  | List all registered tools         |
| `GET`  | `/.well-known/agent.json` | A2A Agent Card (discovery)        |

---

## Features

### 1. Security — CAPSEM (Zero-Trust)

- **Capability-based access control**: every tool invocation verified before execution
- **Default-deny policy engine**: wildcard grant overridable by per-resource and per-action deny rules
- **Rate limiting**: per-subject invocation limits (calls/minute)
- **Deny lists**: resource-level and action-level explicit deny rules
- **ProcessSandbox**: timeout-enforced execution with OS-native resource limits (Windows Job Objects / Unix `RLIMIT_AS`)

```bash
# Denied by CAPSEM policy — returns 403
curl -X POST http://127.0.0.1:3030/execute \
  -d '{"name":"test","actions":[{"tool_name":"dangerous-tool","parameters":{}}]}'
```

### 2. Tool Management

| Transport     | Status                                                             | Notes |
| ------------- | ------------------------------------------------------------------ | ----- |
| **Local**     | In-process: `echo`, `add`, `multiply`, `hash`, `sleep`, `env_info` |
| **HTTP/REST** | POST to arbitrary endpoints with JSON payloads                     |
| **GraphQL**   | Mutation/query transport over HTTP                                 |
| **MCP**       | JSON-RPC tool discovery and execution                              |
| **gRPC**      | Dispatches via HTTP/JSON gateway fallback                          |
| **A2A**       | Delegates to remote agents via `POST /tasks/send`                  |

**Cryptographic attestation**: tools can carry Ed25519 (classical) or ML-DSA / Dilithium3 (post-quantum) signatures verified at registration and invocation.

### 3. Parallel Execution + PRM Pruning

- **DAG scheduling**: topological sort with cycle detection; independent actions execute concurrently
- **Tokio JoinSet**: branches run in parallel — 3 × 100ms tasks complete in ~110ms
- **PRM branch pruning**: heuristic scorer (risk level + parameter completeness + action type) prunes low-confidence branches before execution
- Configurable pruning threshold (default: 0.15)

```bash
# 3 parallel sleep branches — proves concurrency
curl -X POST http://127.0.0.1:3030/execute \
  -d '{"name":"bench","actions":[
    {"tool_name":"sleep","parameters":{"ms":100}},
    {"tool_name":"sleep","parameters":{"ms":100}},
    {"tool_name":"sleep","parameters":{"ms":100}}
  ]}'
# → total_latency_ms: ~110 (not 300)
```

### 4. Observability — Audit Trail

- **BLAKE3 hash-chained events**: every event includes `previous_hash` + `event_hash` for tamper evidence
- **Full event coverage**: `TaskScheduled`, `CapabilityVerified`, `CapabilityDenied`, `ToolInvoked`, `BranchPruned`, `RecoveryAttempted`, `TaskCompleted`
- **Chain verification**: `GET /audit` returns `chain_valid: true` when no tampering detected

### 5. Context Management

- **Salience-weighted compression**: messages scored by role (system > assistant > user), content length, and structured-output markers
- **Recency bonus**: newer messages get a slight score boost before eviction
- **Token budget**: configurable max context size (default 8,000 tokens, ~4 chars/token)

### 6. Recovery

- **Checkpoint/restore**: captures `ExecutionContext` snapshots with BLAKE3 integrity hashes
- **Exponential backoff retry**: `retry_with_backoff` available for tool-level retries
- **Recovery audit events**: `RecoveryAttempted` logged with attempt count and checkpoint count

### 7. A2A Protocol

- **Agent Card**: served at `GET /.well-known/agent.json` — advertises runtime identity, capabilities, and endpoint
- **Task delegation**: `A2aClient` sends tasks to remote agents via `POST /tasks/send` and polls for completion
- **`AgentCommunication` action type**: pipeline routes these actions to the A2A stage automatically

---

## Tool Reference

| Tool             | Transport | Description                               |
| ---------------- | --------- | ----------------------------------------- |
| `echo`           | Local     | Returns input message                     |
| `add`            | Local     | Adds two numbers                          |
| `multiply`       | Local     | Multiplies two numbers                    |
| `hash`           | Local     | BLAKE3 hash of input string               |
| `sleep`          | Local     | Sleeps N ms (parallel benchmark)          |
| `env_info`       | Local     | Returns OS/arch info                      |
| `read_file`      | MCP       | Reads file via MCP filesystem server      |
| `list_files`     | MCP       | Lists directory via MCP filesystem server |
| `write_file`     | MCP       | Writes file via MCP filesystem server     |
| `dangerous-tool` | Local     | Registered but CAPSEM-denied (403 demo)   |

---

## Design Decisions

### Why Rust?

Memory-safe, predictable latency (no GC), true parallelism via Tokio, cross-platform from day one. All competitors (LangGraph, CrewAI) are Python/TypeScript.

### Capability-Based Security (CAPSEM)

Inspired by OS process capabilities and OWASP MCP Top 10. Default-deny with explicit grants. Per-resource deny overrides wildcards. Rate limiting prevents abuse. More granular than RBAC.

### Hash-Chained Audit Logs (BLAKE3)

Each event includes `previous_hash`; recomputing hashes detects tampering. Simpler than merkle trees, sufficient for compliance-grade auditability.

### PRM Branch Pruning

Heuristic scorer evaluates tool risk, parameter completeness, and action type before committing CPU to a branch. Integrates with ML model at the `score_branch()` hook.

### Salience-Weighted Context Compression

Context compressed by importance score (not just age). System prompts preserved longest; short noisy messages evicted first. Prevents token cost explosion in long workflows.

### Post-Quantum Cryptography

Ed25519 for classical deployments; Dilithium3 (ML-DSA) via `pqcrypto-dilithium` for quantum-resistant tool attestation. Algorithm selected per `ToolAttestation.algorithm` field.

---

## Phase Status

| Phase | Features                                                                                                                                                                                    |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | Skeleton, end-to-end pipeline, Axum HTTP server                                                                                                                                             |
| 2     | CAPSEM, Tool Manager, local/HTTP tools                                                                                                                                                      |
| 3     | MCP integration (JSON-RPC, tool discovery)                                                                                                                                                  |
| 4     | DAG parallel execution (Tokio JoinSet, timing proof)                                                                                                                                        |
| 5     | Context compression, runtime metrics                                                                                                                                                        |
| 6     | Audit hardening (BLAKE3), recovery stub                                                                                                                                                     |
| **7** | **PQC attestation, A2A protocol, PRM pruning, GraphQL/gRPC transports, salience compression, rate limiting, per-action deny, checkpoint/restore, full audit coverage, real metrics wiring** |

---

## Testing

```bash
# All unit tests
cargo test --workspace

# Release build
cargo build --release

# Lint
cargo clippy --all

# Format
cargo fmt --all
```

---

**License**: Apache-2.0
