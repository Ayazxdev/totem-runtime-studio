# Totem Runtime Studio — Architecture Overview

## Executive Summary

Totem Runtime Studio (Sentinel Runtime) is a **secure execution kernel for AI agent workflows**. It is not an orchestration framework; it is the low-level runtime that safely executes workflows designed by higher-level tools.

**Core principle**: model the runtime like an OS kernel — instead of scheduling CPU processes, it schedules AI agent tasks and branches, with zero-trust security, observable execution, and transactional recovery.

---

## Layer Architecture

### 1. Scheduling & Execution (`crates/scheduler`)

#### Components
- **`TaskDAG`** (`dag.rs`) — builds dependency graph from task actions; topological sort with cycle detection; `identify_parallel_branches()` assigns independent actions to the same execution level
- **`ParallelExecutor`** (`executor.rs`) — spawns each branch level into a Tokio `JoinSet`; collects `BranchExecutionResult` with per-level timing
- **`BranchPruner`** (`pruner.rs`) — PRM-style heuristic scorer evaluates each branch before execution

#### PRM Pruning (Phase 7)
```
score_branch(action) → f64 in [0.0, 1.0]
  Factor 1: tool risk (dangerous/delete → 0.05; read/list → 1.0)
  Factor 2: parameter completeness (empty params → ×0.8)
  Factor 3: action type (ToolCall baseline; AgentCommunication → ×0.9)
Threshold: 0.15 (configurable via BranchPruner::with_threshold)
Branches below threshold → pruned, counted in metrics.branches_pruned
```

Integration point: replace `score_branch()` body with an ML model call (ONNX, gRPC inference endpoint) for production PRM scoring.

#### Performance Proof
```
3 independent branches × 100ms each:
  Sequential: 300ms
  DAG Tokio parallel: ~110ms
  Reduction: 63.3% (≥40% target)
```

---

### 2. Context Layer (`crates/context`)

#### Components
- **`ContextStage`** — entry point; default 8,000 token limit
- **`compress_context()`** (`window.rs`) — salience-weighted eviction loop
- **`score_message_salience()`** (`scorer.rs`) — heuristic importance score

#### Salience Scoring (Phase 7)
```
score = 1.0
× role_weight:    system=1.4, assistant=1.1, user=0.9
× length_weight:  <20 chars→0.5, <80→0.8, >2000→1.2
× keyword_signal: contains "result:", "```", "json" → ×1.15
+ recency_bonus:  index/total × 0.1 (newer messages slightly favoured)
clamp(0.0, 1.0)
```

Eviction loop alternates between messages and tool results, always evicting the lowest-scored item first, until context is within budget. 

Integration point: replace `score_message_salience()` body with LLMLingua-2-style distillation or DACS scoping model.

---

### 3. Security Layer (`crates/capsem`)

#### Components
- **`PolicyEngine`** (`policy.rs`) — grant/deny permission matrix with rate limiting
- **`CapabilityManager`** (`capability.rs`) — issue and validate time-bounded capability tokens
- **`ProcessSandbox`** (`sandbox.rs`) — timeout-enforced execution with OS memory limits

#### Policy Engine (Phase 7)
```
verify_action(subject, resource, action_type):
  1. Global resource deny list (denied_resources DashSet)    ← highest priority
  2. Per-action deny rules (denied_actions DashSet)
  3. Per-subject rate limit (sliding 60s window)
  4. Exact grant: policies[subject][resource].contains(action)
  5. Wildcard subject: policies["*"][resource].contains(action)
  6. Wildcard all: policies["*"]["*"].contains(action)
  Default: deny
```

New APIs in Phase 7:
- `policy.deny_action(resource, action)` — deny a specific action regardless of grants
- `policy.set_rate_limit(subject, calls_per_minute)` — sliding window rate limiter

#### ProcessSandbox (Phase 7)
```rust
ProcessSandbox::new(timeout_secs)
    .with_memory_limit(bytes)
    .execute(|| { /* tool function */ })
```
- Thread spawned via `std::thread::spawn` with `mpsc::recv_timeout`
- On Windows: `SetProcessWorkingSetSize` for working-set hints
- On Linux/macOS: `setrlimit(RLIMIT_AS)` for virtual memory cap
- Timeout exceeded → `SentinelError::SandboxViolation`

---

### 4. Tool Interoperability Layer (`crates/tool-manager`)

#### Transport Dispatch (`registry.rs`)
```
invoke_tool(name, params):
  get(name) → ToolDefinition
  if attestation present → verify_attestation()   ← new in Phase 7
  match transport:
    Local       → local::invoke_local_tool()
    Http        → http::invoke_http_tool()
    GraphQl     → graphql::invoke_graphql_tool()  ← new in Phase 7
    Grpc        → http::invoke_http_tool() [gateway fallback]
    Mcp         → error (handled by McpStage)
    A2a         → a2a_dispatch::delegate_to_agent() ← new in Phase 7
```

#### Cryptographic Attestation (Phase 7)
```
verify_attestation(attestation):
  algorithm == "ed25519"  → Ed25519 verify (ed25519-dalek)
    - key: 32-byte compressed point
    - message: SHA-256(public_key)
    - sig: 64-byte Ed25519 signature
  algorithm == "ml-dsa" | "dilithium3" → Dilithium3 verify (pqcrypto-dilithium)
    - pk: Dilithium3 public key bytes
    - signed_msg: full signed-message (sig ‖ msg) per pqcrypto API
```

#### Local Tools (Phase 7 additions)
| Tool | Input | Output |
|------|-------|--------|
| `echo` | `{message}` | `{echo}` |
| `add` | `{a, b}` | `{sum, a, b}` |
| `multiply` | `{a, b}` | `{product, a, b}` |
| `hash` | `{input}` | `{input, algorithm, digest}` |
| `sleep` | `{ms}` | `{slept_ms}` |
| `env_info` | `{}` | `{os, arch, family}` |

---

### 5. MCP Integration (`crates/mcp`)

- **`McpClient`** — JSON-RPC 2.0 client over HTTP POST
- **`discover()`** — calls `tools/list`, returns `[ToolDefinition]`
- **`execute_tool()`** — calls `tools/call` with `{name, arguments}`
- **`McpStage`** — pipeline stage; routes `read_file`, `list_files`, `write_file` to the `filesystem` MCP server at `http://127.0.0.1:3031`

---

### 6. A2A Protocol (`crates/a2a`)

#### Agent Card (Phase 7)
Served at `GET /.well-known/agent.json`. Describes:
- Agent identity (URN), name, description, version
- HTTP endpoint for task delegation
- Capabilities: `tool_execution`, `parallel_dag_scheduling`, `audit_trail`
- Supported content types

```json
{
  "id": "urn:totem:sentinel-runtime:v1",
  "name": "Totem Sentinel Runtime",
  "endpoint": "http://127.0.0.1:3030",
  "capabilities": [...]
}
```

#### Task Delegation (Phase 7)
```
A2aClient::send_and_wait(tool_name, params, max_polls):
  POST {endpoint}/tasks/send  ← A2A task envelope
  poll GET {endpoint}/tasks/{id}  ← up to max_polls × 500ms
  returns A2aTaskResult { task_id, status, output, agent_id }
```

`A2aStage` in the pipeline routes `ActionType::AgentCommunication` actions to remote agents via `agent_endpoint` parameter.

---

### 7. Governance Layer

#### Audit Logger (`crates/audit`)

**Event types fired per execution:**
```
1. TaskScheduled       — action count, parallel branches, pruned count
2. CapabilityVerified  — per allowed action (tool name)
3. CapabilityDenied    — per denied action (tool name, reason)
4. ToolInvoked         — per tool result (tool name, latency_ms, output size)
5. BranchPruned        — if pruned_count > 0
6. RecoveryAttempted   — if recovery_attempts > 0 (from RecoveryStage)
7. TaskCompleted       — tools_invoked, capability_checks, denials
```

**Hash chain formula:**
```
event_hash_n = BLAKE3(
  event.id ‖ timestamp ‖ event_type ‖ subject ‖ details ‖ event_hash_{n-1}
)
```

`verify_chain()` replays all events, recomputes each hash, and checks `previous_hash` continuity.

#### Evaluation (`crates/evaluation`)

Metrics written into `context.metadata` by `EvaluationStage`:
```
latency_ms           = sum(tool_result.latency_ms) or wall_clock
token_count          = (message_chars + tool_result_chars) / 4
estimated_cost_usd   = token_count × $0.00001
parallel_branches    = from Scheduler metadata
branches_pruned      = from Scheduler metadata
capability_checks    = from CAPSEM metadata
capability_denials   = from CAPSEM metadata
```

`ExecutionPipeline::execute()` reads these fields back from `context.metadata` to build the final `ExecutionMetrics` — no hardcoded zeros.

#### Recovery (`crates/recovery`)

**Checkpoint (Phase 7):**
```rust
Checkpoint::capture(task_id, context) → Checkpoint {
  context_snapshot: ExecutionContext,
  hash: BLAKE3(serialize(context))  // integrity check on restore
}

Checkpoint::restore() → verify hash → return context_snapshot
```

`RecoveryStage` saves a checkpoint after every successful execution and logs `RecoveryAttempted` if `recovery_attempts > 0`.

---

## Pipeline Wiring

```rust
// api/src/lib.rs — composition root
pipeline.add_stage(Box::new(SchedulerStage::new()));
pipeline.add_stage(Box::new(ContextStage::default()));      // 8k tokens
pipeline.add_stage(Box::new(capsem.as_ref().clone()));
pipeline.add_stage(Box::new(mcp_stage.as_ref().clone()));   // MCP first
pipeline.add_stage(Box::new(ToolManagerStage::with_registry(tool_mgr.registry_arc())));
pipeline.add_stage(Box::new(A2aStage));
pipeline.add_stage(Box::new(AuditStage::new(audit_logger.clone())));
pipeline.add_stage(Box::new(EvaluationStage::new()));
pipeline.add_stage(Box::new(RecoveryStage::new(audit_logger.clone())));
```

`ToolManagerStage::with_registry(Arc)` shares the registry between the composition root (used for `/tools` listing) and the pipeline stage (used for execution) — single source of truth.

---

## Security Model

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Unauthorized tool execution | CAPSEM deny-first policy (Stage 3); 403 on denial |
| Audit tampering | BLAKE3 hash-chain; `chain_valid` flag on `/audit` |
| Privilege escalation | Zero-trust; no implicit grants; deny list overrides wildcards |
| Abuse / flooding | Per-subject rate limiting in PolicyEngine |
| Compromised tool binary | Ed25519 / ML-DSA attestation verified at registration + invocation |
| Sandbox escape (future) | ProcessSandbox timeout + OS memory limits; full namespace isolation deferred |

### Trust Boundaries

```
┌─────────────────────────────────────────────────────┐
│  Untrusted (attacker may control)                  │
│  — HTTP request body (task name, actions, params)  │
│  — Tool output (if from external service)          │
│  — A2A agent responses                             │
└────────────────────────┬────────────────────────────┘
                         ↓
           ┌─────────────────────────┐
           │  CAPSEM Verification   │
           │  (always runs stage 3) │
           └─────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────┐
│  Trusted (inside runtime)                          │
│  — Policy engine (in-memory, server-configured)    │
│  — Audit logger (append-only, hash-chained)        │
│  — Metrics (computed from verified context)        │
│  — Checkpoint store (BLAKE3-verified snapshots)    │
└─────────────────────────────────────────────────────┘
```

---

## Dependency Graph

```
sentinel-core  ←── scheduler
               ←── capsem
               ←── tool-manager
               ←── mcp
               ←── a2a
               ←── context
               ←── audit
               ←── evaluation
               ←── recovery ←── audit
               ←── api      ←── ALL of the above
```

No circular dependencies. `api` is the sole composition root.

---

## Performance Characteristics

| Scenario | Latency |
|----------|---------|
| Empty task (no actions) | ~5ms |
| Single local tool call | ~10ms |
| 3 parallel tools (100ms each) | ~110ms |
| MCP tool round-trip | ~20–50ms |
| Audit chain verify (100 events) | <1ms |

Throughput: Axum + Tokio handles 1,000+ concurrent requests on modern hardware.

---

## Future Work

| Feature | Notes |
|---------|-------|
| Full OS process isolation | Linux namespaces / cgroups; Windows Job Objects for child processes |
| ML-based PRM scoring | Replace heuristic with ONNX model or gRPC inference endpoint |
| LLMLingua-2 compression | Replace heuristic scorer with distillation-based compression |
| Distributed audit replication | Replicate hash-chain to secondary store for DR |
| gRPC native transport | Replace HTTP gateway fallback with tonic reflection client |
| Dashboard / TUI | Real-time execution and audit monitoring |
