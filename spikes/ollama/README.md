# Spike: Ollama local inference (P0-T03)

Throwaway. Not a workspace member; product code never imports it. The real adapter is
`crates/loki-llm/src/ollama.rs` (P2-T01).

## Goal

On the dev machine, show streaming and tool calling through Ollama's `/api/chat`, measure cold
load, first-token latency, tokens/s and peak RAM, check that tool calls are well-formed at least
9 times in 10, and test cancellation by dropping the connection mid-stream.

## Environment (2026-11-27)

MacBook Air, Apple M2 (8 cores), 8 GB unified memory, macOS 27.0.1. Ollama 0.40.18 (Homebrew),
Metal backend, `default_num_ctx=4096` picked by Ollama for 5.3 GiB usable VRAM. Rust 1.97.1.
Full details are in [docs/p0/environment.md](../../docs/p0/environment.md). No git commit: the
repo's `.git` is broken (see STATUS.md).

## Reproduce

```sh
# terminal 1: Ollama on loopback with cloud features off (same as scripts/ollama-serve.sh)
OLLAMA_HOST=127.0.0.1:11434 OLLAMA_NO_CLOUD=1 ollama serve > spikes/ollama/output/server.log 2>&1

# terminal 2
ollama pull qwen3:4b          # 2.5 GB, 84 s on this connection
ollama pull bonsai-2:4b       # 1.6 GB, 44 s
ollama show qwen3:4b --license | head -3      # Apache License 2.0
ollama show bonsai-2:4b --license | head -3   # Apache License 2.0
cd spikes/ollama
cargo build --release
./target/release/ollama-spike qwen3:4b      # ~6 min; writes output/qwen3-4b.json
curl -s 127.0.0.1:11434/api/generate -d '{"model":"qwen3:4b","keep_alive":0}'   # unload
./target/release/ollama-spike bonsai-2:4b   # ~2 min
grep 'stop: cancel task' output/server.log  # server-side proof of cancellation
```

Every request uses `think: false`, `options.num_ctx = 4096`, streaming, and the model's default
sampling parameters (qwen3: temperature 0.6, top_k 20, top_p 0.95). Nothing else was running
except the editor.

## Results

| Metric | qwen3:4b (Q4_K_M) | bonsai-2:4b (Q2_0 ternary) |
| --- | --- | --- |
| Download size | 2.5 GB | 1.6 GB (includes a 334M vision projector) |
| Loaded size (`/api/ps`, num_ctx 4096) | 3.18 GB, 100% on GPU | 1.72 GB, 100% on GPU |
| Peak RSS, all ollama processes | 3,128 MB | 1,943 MB |
| Cold load (`load_duration`) | 2.61 s | 3.40 s |
| Cold time to first token (wall) | 2.74 s | 3.89 s |
| Warm time to first token, median of 5 (max) | 0.080 s (0.146) | 0.110 s (0.445) |
| Generation speed, median of 5 (min) | 30.0 tok/s (29.7) | 20.7 tok/s (20.4) |
| Prompt processing, median | 735 tok/s | 269 tok/s |
| Tool call well-formed, tool named `time.now` | **10/10** (10 with `Asia/Tokyo`) | **10/10** (10 with `Asia/Tokyo`) |
| Tool call well-formed, tool named `time_now` | **10/10** (10) | **10/10** (10) |
| Spurious tool calls on 10 no-tool prompts | 0/10 | 0/10 |
| Tool round trip (result fed back) | Used the result, **but leaked reasoning into `content`** ("Okay, let me process this…") despite `think: false` | Clean: "It is currently **00:31:00** on November 28, 2026, in Tokyo." |
| Cancel by dropping the stream | After 20 chunks; next request's first token in 0.25 s | *Not exercised*: the reply ended after 7 chunks, before the drop point |
| Cancel by token while blocked in `next()` | Loop exited 0.014 ms after `cancel()`; next first token 0.22 s | 0.012 ms; next first token 0.15 s |
| Server-side abort logged | yes, `srv stop: cancel task` | yes |
| Cloud model with `OLLAMA_NO_CLOUD=1` | HTTP 403 "ollama cloud is disabled: remote model is unavailable" | same |

"Well-formed" means exactly one call, the exact tool name, object arguments, no unknown keys,
and an IANA-shaped `timezone`. Tool calls arrived as a single streamed chunk carrying the full
call (not split across chunks).

## Findings that change the plan

1. **Both models pass the 9/10 bar**, with dotted names too: `ToolSpec` names can be sent
   unchanged, with no `time.now` to `time_now` mapping needed (for these two models).
2. **Cancellation works by dropping the HTTP connection.** llama.cpp logs the cancel, and the
   next request starts immediately. The P2-T02 design (cancel token drops the body stream) is right.
3. **`think: false` isn't a guarantee.** qwen3 put reasoning text in `content` after a tool
   result. P2/P3 must not assume content is free of reasoning; a quality test should cover it.
4. **Ollama's defaults aren't private enough for Loki**: cloud models are on
   (`OLLAMA_NO_CLOUD=false`), and the first `ollama serve` creates a key pair at
   `~/.ollama/id_ed25519`. Loki's docs and `scripts/ollama-serve.sh` set `OLLAMA_NO_CLOUD=1`.
   The adapter must also refuse `:cloud` / remote models itself, because Loki can't control how
   the user started Ollama.
5. **Ollama's default `OLLAMA_ORIGINS` includes `tauri://*`** and other local origins. That
   matters for P5: the webview could reach Ollama directly. Loki's UI must go through the runtime.
6. Memory on 8 GB: qwen3:4b uses about 3.2 GB while loaded. Fine for Loki alone, but tight next
   to a browser and an IDE. bonsai-2:4b needs about 1.9 GB.
7. On Apple Silicon, RSS includes the Metal buffers (RSS ≈ `/api/ps` size), so RSS is a usable
   memory metric here. The runner process is `llama-server`.

## Conclusion

Recommended default for 8 GB machines: **`qwen3:4b`** (Apache-2.0): fastest (30 tok/s, 80 ms
warm first token), with perfect tool-call results. **`bonsai-2:4b`** (Apache-2.0) is the
low-memory alternative (1.9 GB): cleaner after a tool result, but about 30% slower and newer, so
less proven. Recorded in `fixtures/models.json`; the formal decision is ADR-0009.

## Limitations

- One prompt type for tool calls, 10 trials per name: enough for the 9/10 gate, not a quality study.
- Single machine, a single run per model, model-default sampling.
- The drop-the-stream cancellation case for bonsai wasn't exercised (see above).
- The spike ran while an editor was open; no other heavy processes.

## Addendum (2026-11-27, after the first real chat in P2)

The tables above are unchanged. One thing the spike didn't check, output cleanliness, turned out
to matter most. On Ollama 0.40.18, **qwen3:4b with `think: false` writes its reasoning into
`content`** on ordinary questions, not only after tool results. With `think: true` the reasoning
comes back separately and the reply is clean. bonsai-2:4b with `think: false` is clean and fast.
The default moved to bonsai-2:4b, and thinking became a per-model setting: see ADR-0010, which
supersedes ADR-0009. The comparison commands are in that ADR.
