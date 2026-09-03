# Totem Runtime Studio (Sentinel Runtime)

**A secure, local-first, Rust-based execution runtime for autonomous AI agent workflows.**

Totem Runtime Studio is **not** another workflow builder. It is the **secure execution kernel** that runs underneath orchestration tools — comparable to how an OS kernel manages processes while higher-level tools manage them. Totem executes agent workflows **securely, observably, and reliably**.

---

## Positioning

| **Workflow Builders** | **Totem Runtime Studio** |
|---|---|
| Design and compose workflows | **Execute workflows securely** |
| LangGraph, CrewAI, n8n, Flowise | **Sentinel Runtime** |
| "What should the agent do?" | "How do we safely run it?" |

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

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/execute` | Execute a task workflow |
| `GET` | `/audit` | Retrieve hash-chained audit trail |
| `GET` | `/health` | Health check + version |
| `GET` | `/tools` | List all registered tools |
| `GET` | `/.well-known/agent.json` | A2A Agent Card (discovery) |

### `POST /execute`

```json
{
  "name": "task-name",
  "description": "optional",
  "actions": [
    {
      "tool_name": "echo",
      "parameters": {"message": "hello"},
      "action_type": "tool_call"
    }
  ]
}
```

`action_type` options: `"tool_call"` (default), `"agent_communication"`, `"context_update"`

**Response:**
```json
{
  "task_id": "uuid",
  "status": "success",
  "output": { "tool_results": [...] },
  "metrics": {
    "total_latency_ms": 110,
    "parallel_branches_executed": 3,
    "branches_pruned": 0,
    "total_token_count": 42,
    "estimated_cost_usd": 0.00042,
    "tools_invoked": 3,
    "capability_checks": 3,
    "capability_denials": 0,
    "recovery_attempts": 0
  },
  "audit_trail_id": "uuid"
}
```

### `GET /audit`
```json
{
  "events": [...],
  "chain_valid": true,
  "event_count": 26
}
```

### `GET /.well-known/agent.json`
Returns the A2A Agent Card — machine-readable identity and capability advertisement per the Google Agent2Agent Protocol v1.0 / Linux Foundation spec.

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
| Transport | Status | Notes |
|-----------|--------|-------|
| **Local** | ✅ | In-process: `echo`, `add`, `multiply`, `hash`, `sleep`, `env_info` |
| **HTTP/REST** | ✅ | POST to arbitrary endpoints with JSON payloads |
| **GraphQL** | ✅ | Mutation/query transport over HTTP |
| **MCP** | ✅ | JSON-RPC tool discovery and execution |
| **gRPC** | ✅ | Dispatches via HTTP/JSON gateway fallback |
| **A2A** | ✅ | Delegates to remote agents via `POST /tasks/send` |

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

| Tool | Transport | Description |
|------|-----------|-------------|
| `echo` | Local | Returns input message |
| `add` | Local | Adds two numbers |
| `multiply` | Local | Multiplies two numbers |
| `hash` | Local | BLAKE3 hash of input string |
| `sleep` | Local | Sleeps N ms (parallel benchmark) |
| `env_info` | Local | Returns OS/arch info |
| `read_file` | MCP | Reads file via MCP filesystem server |
| `list_files` | MCP | Lists directory via MCP filesystem server |
| `write_file` | MCP | Writes file via MCP filesystem server |
| `dangerous-tool` | Local | Registered but CAPSEM-denied (403 demo) |

---

## Architecture

### Pipeline Stage Order

```
POST /execute
    ↓
[Scheduler]      DAG decomposition, parallel branch identification, PRM pruning
    ↓
[Context]        Salience-weighted compression to token budget
    ↓
[CAPSEM]         Zero-trust capability checks; rate limit; deny rules
    ↓
[MCP]            Execute MCP tools via JSON-RPC
    ↓
[ToolManager]    Execute local / HTTP / GraphQL / gRPC / A2A tools
    ↓
[A2A]            Delegate AgentCommunication actions to remote agents
    ↓
[Audit]          Hash-chain all events (BLAKE3)
    ↓
[Evaluation]     Compute metrics → write to context.metadata
    ↓
[Recovery]       Checkpoint context; log recovery attempts
    ↓
ExecutionResult  ← real metrics read from context.metadata
```

### Crate Structure

```
crates/
├── sentinel-core/      Shared types, traits, pipeline abstraction
├── scheduler/          DAG, topological sort, parallel executor, PRM pruner
├── capsem/             Capability tokens, policy engine, sandbox
├── tool-manager/       Registry, local/HTTP/GraphQL/gRPC/A2A transports, PQC attestation
├── mcp/                MCP JSON-RPC client, discovery, execution
├── a2a/                Agent Card, A2A HTTP client, pipeline stage
├── context/            Salience scorer, sliding-window compression
├── audit/              BLAKE3 hash-chained immutable event logger
├── evaluation/         Runtime metrics computation
├── recovery/           Retry logic, checkpoint/restore
├── api/                Axum HTTP server, /tools, /.well-known/agent.json
└── mcp-test-server/    Reference MCP server for testing
```

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

| Phase | Status | Features |
|-------|--------|----------|
| 1 | ✅ | Skeleton, end-to-end pipeline, Axum HTTP server |
| 2 | ✅ | CAPSEM, Tool Manager, local/HTTP tools |
| 3 | ✅ | MCP integration (JSON-RPC, tool discovery) |
| 4 | ✅ | DAG parallel execution (Tokio JoinSet, timing proof) |
| 5 | ✅ | Context compression, runtime metrics |
| 6 | ✅ | Audit hardening (BLAKE3), recovery stub |
| **7** | ✅ | **PQC attestation, A2A protocol, PRM pruning, GraphQL/gRPC transports, salience compression, rate limiting, per-action deny, checkpoint/restore, full audit coverage, real metrics wiring** |

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

## Security Considerations

### Secured
✅ Tool invocation requires capability verification  
✅ Deny-first policy with rate limiting and per-action deny rules  
✅ Audit trail is immutable (BLAKE3 hash-chain)  
✅ Tool attestation (Ed25519 + ML-DSA / Dilithium3)  
✅ ProcessSandbox with timeout and OS-native memory limits  

### Not Yet Secured (future work)
🔜 Full OS-level process isolation (Linux namespaces, Windows Job Objects for child processes)  
🔜 Prompt injection detection  
🔜 Distributed audit log replication  

---

## References

- **A2A Protocol** — Google Agent2Agent v1.0 / Linux Foundation
- **MCP** — Anthropic Model Context Protocol
- **AIOS** — LLM Agent Operating System (COLM 2025)
- **NIST PQC** — ML-KEM, ML-DSA (FIPS 203/204)
- **OWASP MCP Top 10** — security guidance for MCP deployments
- **LLMLingua-2** — prompt compression via data distillation
- **PRM** — Process Reward Models for branch scoring
- **TEA** — Transactional Execution Architecture

---

**License**: MIT OR Apache-2.0
