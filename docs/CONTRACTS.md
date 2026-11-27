# Agent and tool contracts

The Rust source of truth is `crates/loki-core`. TypeScript types are generated into
`packages/contracts` (from P1-T03). **Changing any shape here is a contract change.** Update this
file, the Rust types and the generated TS in the same commit.

## Core types (implemented in `loki-core`)

```text
ToolRequest   { id, session_id, tool, arguments, deadline }        deny_unknown_fields
ToolSpec      { name, description, input_schema, risk, capability } deny_unknown_fields
ToolResult    { id, status, data?, error_code?, duration_ms }         deny_unknown_fields
Message       { id, conversation_id, role, content, created_at, stopped }   deny_unknown_fields (stopped: P2-T02)
Conversation  { id, title, created_at, updated_at }                deny_unknown_fields
Role          = System | User | Assistant | Tool
IDs           RequestId, SessionId, ToolName, ConversationId, MessageId   (string newtypes; UUID v7 text, ADR-0004)
UnixMillis    i64 ms since the epoch, UTC (TS: number)
Grant         { capability, scope }                     deny_unknown_fields; scope "*" or an allowed-root ID
NetworkMode   = LocalOnly | ToolsAllowed               (moved here from loki-runtime, ADR-0011)
TaskId, ApprovalId                                     string newtypes (P3)
Risk          = ReadOnly < Network < LocalChange < Sensitive
ToolStatus    = Succeeded | Failed | Cancelled | TimedOut | Denied
ErrorCode     = InvalidArguments | UnknownTool | PolicyDenied | ApprovalInvalid | PermissionRequired
              | CapabilityUnavailable | Timeout | Cancelled | ResultTooLarge | Offline | ExecutionFailed
Decision      = Allow | Deny { reason } | RequireApproval              tag "decision", deny_unknown_fields
TaskState     = Queued | Running | AwaitingApproval | Succeeded | Failed | Cancelled | Interrupted
Capability    = Clock | SystemInfo | AppLaunch | FileRead | Network | Microphone | Notifications | DeveloperTools
CapabilityStatus = Supported | PermissionRequired{reason} | Denied{reason} | Unavailable{reason}   tag "status", deny_unknown_fields
```

Every type above has a serde round-trip test, and an unknown-field rejection test where
`deny_unknown_fields` applies. All enums serialize as `snake_case`.

### TypeScript

Generated with ts-rs into `packages/contracts/src/generated/` (ADR-0005). Regenerate with
`./scripts/gen-contracts.sh`; CI fails if the committed files are stale (`--check`) or don't
compile under `tsc --strict`.

Task transitions are enforced by `TaskState::can_transition_to`; the diagram is in `crates/loki-core/src/task.rs`.

## Interfaces (to be implemented in the phase shown)

```text
ChatProvider (loki-llm, implemented in P1-T04, ADR-0006). Object-safe: Arc<dyn ChatProvider>.
  name() -> &str
  stream_chat(ChatRequest { messages: Vec<ChatMessage { role, content }>, tools: Vec<ToolSpec> },
              cancel: CancellationToken) -> BoxStream<'static, Result<ChatEvent, LlmError>>
      ChatEvent = Token { text } | ToolCall(ToolCallDraft { tool, arguments }) | Done(Usage { prompt_tokens?, completion_tokens? })
      LlmError  = Cancelled | Unavailable(msg) | ModelMissing(msg) | Protocol(msg) | ContextLimit(msg)
      Protocol: content items, then exactly one terminal item (Ok(Done) or Err), then the stream ends.
                Cancellation ends it promptly with Err(Cancelled).
  health() -> BoxFuture<ProviderHealth = Ready | Unavailable { reason }>
  context_limit() -> u32 tokens

  MockProvider (tests and `--provider mock`): scripted from JSON (fixtures/mock_provider/) or `echo()`.
  OllamaProvider (P2-T01): `/api/chat` NDJSON; the HTTP client is injected by loki-runtime::net
      (loopback IP literal only, no proxy, loopback-only redirects). `think` is set per model from
      fixtures/models.json (ADR-0010); `thinking` text is always dropped. Tool names are sent unchanged.
      Health = `/api/version` + the model present in `/api/tags`. Cloud model tags are refused by the runtime.

Agent (loki-agent, P2-T03/T04)
  build_context(system, history, budget): system prompt (at most half the budget), then the latest
      user turn (always; truncated with a marker if needed), then the newest turns that fit.
      Tokens estimated as ceil(chars/4) + 4 per message. Budget = context_limit - reserve.
  Orchestrator::send / retry / reply over traits ChatStore + IdSource. A stopped reply with text is
      stored with stopped = true; an empty one is not stored. No tools are offered in P2.

Platform (loki-platform, P3-T07)
  capabilities() -> Map<Capability, CapabilityStatus>
  list_apps() -> Vec<AppEntry { app_id, display_name }>
  launch_app(app_id) -> LaunchOutcome (verified)
  system_info() -> SystemInfo

Tool (loki-tools, implemented in P3-T01)
  spec() -> &ToolSpec
  scope(args) -> String                       grant scope for policy (`*`, or a root ID for files.*)
  execute(args, ToolContext { cancel, deadline, platform }, permit: &loki_policy::Allowed)
      -> BoxFuture<Result<Value, ToolError>>    the permit makes skipping policy a compile error (ADR-0011)
  audit_summary(&Value) -> String             redacted one-liner, never output data

Executor (loki-tools, P3-T04)
  run(tool, request, permit: Allowed (by value, not Clone), platform, cancel) -> Execution { result, audit_summary }
      checks permit.matches(request); semaphore; deadline; cancel; 64 KiB result cap

Policy (loki-policy, implemented in P3-T02/T03)
  evaluate(PolicyInput { request, spec?, capability_status?, scope, grants, network_mode }) -> Decision
      order: unknown tool -> capability not Supported -> no grant for (capability, scope) ->
             Network in local_only -> LocalChange/Sensitive need approval -> Allow
  authorize(input, approval_id, now, ttl) -> Allowed | NeedsApproval(PendingApproval) | Denied { reason }
  allow_approved(ConsumedApproval, request) -> Result<Allowed, ApprovalError>   (store consumed it first)

Agent tool loop (loki-agent, P3-T05)
  validate -> policy -> approval (ToolUi, default no) -> execute -> audit; results go back wrapped as
  <tool_result untrusted="true" tool="...">...</tool_result>; max 6 calls per turn (config, hard max 20)
```

**P1-T04 decision (ADR-0006):** errors travel only as `Err(LlmError)` stream items; there is no
`ChatEvent::Error` variant. A `ToolCallDraft` has no request ID or deadline; the agent assigns them
when it builds a `ToolRequest`. Until P3, the agent counts and logs tool-call proposals and never runs them.

## Initial tool registry

| Tool | Arguments | Risk | Capability | Behaviour |
| --- | --- | --- | --- | --- |
| `time.now` | `timezone?` (IANA) | ReadOnly | Clock | Returns an ISO-8601 timestamp. |
| `system.info` | none | ReadOnly | SystemInfo | Returns OS, CPU, RAM and disk summary. |
| `app.list` | `query?` (max 64), `limit?` (1-50) | ReadOnly | AppLaunch | Discovered apps (`app_id`, name), so the model can find a valid ID (added in P3-T07). |
| `app.open` | `app_id` | LocalChange | AppLaunch | `app_id` must come from the discovered registry. Verifies the launch. |
| `files.search` | `root_id`, `query`, `limit` (max 50) | ReadOnly | FileRead | Searches inside an approved root only. Returns `file_id`s, never absolute paths. |
| `files.read` | `file_id`, `max_bytes` (max 256 KiB) | ReadOnly | FileRead | Reads supported text types only. |
| `web.search` | `query`, `limit` (max 10) | Network | Network | Needs network opt-in. Returns title, URL, snippet and timestamp for citations. |
| `dev.docker_status` | none | ReadOnly | DeveloperTools | Read-only container status. Never restarts anything. |

## Rules for every tool

- **Strict JSON Schema**: `"additionalProperties": false`, explicit types, bounded strings and
  numbers. A registry test fails if any spec breaks this.
- **Reject unknown fields** at every layer.
- **Cap result size.** The default is 64 KiB of JSON; anything larger is truncated with a marker
  or fails with `ResultTooLarge`.
- **Bound the agent loop** to 6 tool calls per turn initially (configurable, with a hard maximum).
- **Never treat model output as executable code.**
- **Approval binding:** `approval = { request_id, args_hash = sha256(canonical_json({tool, arguments})), expires_at, used }`.
  Changing the arguments changes the hash, which invalidates the approval. Approvals are single use.
- **Verified outcome:** a tool reports `Succeeded` only after checking the effect. For example,
  `app.open` confirms the process started; if it can't confirm, it reports that honestly.

## Adding a new tool (checklist)

1. Add a `ToolSpec`: name, description, schema, risk and capability. If a new capability is needed, add it to `loki-core` (a contract change).
2. Implement `Tool` in `loki-tools`, with OS access via `loki-platform`.
3. Write tests: valid call, invalid arguments, unknown field, denied by policy, cancelled, timeout, oversize result.
4. Add security tests if it touches files, network or other devices (see SECURITY_MODEL).
5. Add a row to the registry table above.
6. If the tool is `LocalChange` or `Sensitive`, make sure the approval preview shows every argument.
