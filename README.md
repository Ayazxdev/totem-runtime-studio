# Totem Runtime Studio

**A secure, local-first, Rust-based execution runtime for autonomous AI agent workflows.**

It is the **secure execution kernel** that runs underneath orchestration tools — comparable to how an OS kernel manages processes while higher-level tools manage them. Totem executes agent workflows **securely, observably, and reliably**.

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

## Positioning

| **Workflow Builders**           | **Totem Runtime Studio**       |
| ------------------------------- | ------------------------------ |
| Design and compose workflows    | **Execute workflows securely** |
| LangGraph, CrewAI, n8n, Flowise | **Sentinel Runtime**           |
| "What should the agent do?"     | "How do we safely run it?"     |

The runtime addresses problems that workflow builders cannot: **runtime security, deterministic execution, context explosion prevention, intelligent scheduling, tool verification, auditability, rollback/recovery, and production reliability.**

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

Context compressed by importance score. System prompts preserved longest; short noisy messages evicted first. Prevents token cost explosion in long workflows.

### Post-Quantum Cryptography

Ed25519 for classical deployments; Dilithium3 (ML-DSA) via `pqcrypto-dilithium` for quantum-resistant tool attestation. Algorithm selected per `ToolAttestation.algorithm` field.

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
