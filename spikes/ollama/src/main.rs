//! P0-T03 Ollama spike: streaming, tool calling, cancellation, latency and memory.
//!
//! Throwaway (see spikes/README.md). Talks to a local Ollama on 127.0.0.1:11434.
//!
//!     cargo run --release -- qwen3:4b
//!
//! For one model it measures:
//!  1. cold load: unload, then time the first request (wall clock + Ollama's `load_duration`)
//!  2. warm streaming x5: time to first token, generation tok/s, prompt tok/s
//!  3. peak RAM: RSS of every `ollama` process, sampled every 100 ms, plus `/api/ps` size
//!  4. tool calls: 10 trials with the tool named `time.now`, 10 with `time_now`
//!  5. false positives: 10 prompts that need no tool, with the tool still offered
//!  6. round trip: feed a tool result back and get the final answer
//!  7. cancellation: (a) drop the stream after 20 chunks, (b) cancel a pending read with a
//!     CancellationToken; then time the next request to see whether the server really stopped
//!  8. cloud refusal: a `:cloud` model must fail when the server runs with OLLAMA_NO_CLOUD=1
//!
//! Results go to `output/<model>.json`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const BASE: &str = "http://127.0.0.1:11434";
const TRIALS: usize = 10;
const WARM_RUNS: usize = 5;
const NUM_CTX: u32 = 4096; // bounded, so memory numbers are comparable on an 8 GB machine
const SYSTEM: &str = "You are Loki, a local assistant. Use a tool only when it is needed to answer.";
const TIME_PROMPT: &str = "What time is it right now in Tokyo?";
const NO_TOOL_PROMPTS: [&str; 10] = [
    "Write a two-line poem about rain.",
    "What is 17 times 3?",
    "Give me a synonym for 'happy'.",
    "Translate 'good morning' into French.",
    "Name three primary colors.",
    "What is the capital of Italy?",
    "Explain what a noun is in one sentence.",
    "Say hello in a friendly way.",
    "Is a tomato a fruit or a vegetable? One sentence.",
    "List two uses of a paperclip.",
];

// ------------------------------------------------------------------------------------ HTTP

/// What one streamed `/api/chat` call produced.
#[derive(Debug, Default)]
struct Record {
    status: u16,
    ttft: Option<Duration>,
    wall: Duration,
    text: String,
    thinking: String,
    tool_calls: Vec<Value>,
    tool_call_chunk_index: Option<usize>,
    chunks: usize,
    content_chunks: usize,
    final_chunk: Option<Value>,
    error: Option<String>,
    stopped_early: bool,
    /// For token cancellation: time from `cancel()` to the loop exiting.
    cancel_to_exit: Option<Duration>,
}

/// How a streaming call should end early, if at all.
enum Stop {
    Never,
    /// Drop the response after this many content chunks.
    AfterContentChunks(usize),
    /// Stop when the token fires; the reader is blocked in `next()` at that moment.
    Token(CancellationToken, Arc<AtomicU64>),
}

fn chat_body(model: &str, messages: Value, tools: Option<Value>, extra_options: Value) -> Value {
    let mut options = json!({ "num_ctx": NUM_CTX });
    if let (Some(base), Some(extra)) = (options.as_object_mut(), extra_options.as_object()) {
        base.extend(extra.clone());
    }
    let mut body = json!({ "model": model, "messages": messages, "think": false, "options": options, "stream": true });
    if let Some(tools) = tools {
        body["tools"] = tools;
    }
    body
}

async fn stream_chat(http: &reqwest::Client, body: &Value, stop: Stop) -> Result<Record> {
    let started = Instant::now();
    let response = http.post(format!("{BASE}/api/chat")).json(body).send().await?;
    let mut record = Record {
        status: response.status().as_u16(),
        ..Record::default()
    };
    if !response.status().is_success() {
        record.error = Some(response.text().await.unwrap_or_default().chars().take(300).collect());
        record.wall = started.elapsed();
        return Ok(record);
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    'read: loop {
        let next = match &stop {
            Stop::Token(token, cancelled_at) => tokio::select! {
                biased;
                () = token.cancelled() => {
                    let at = cancelled_at.load(Ordering::SeqCst);
                    record.cancel_to_exit = Some(Duration::from_nanos(now_nanos().saturating_sub(at)));
                    record.stopped_early = true;
                    break 'read;
                }
                next = stream.next() => next,
            },
            _ => stream.next().await,
        };
        let Some(bytes) = next else { break };
        buffer.extend_from_slice(&bytes?);
        // Ollama streams NDJSON: one JSON object per line.
        while let Some(newline) = buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=newline).collect();
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let chunk: Value = serde_json::from_slice(&line).context("bad NDJSON line")?;
            record.chunks += 1;
            if let Some(error) = chunk.get("error") {
                record.error = Some(error.to_string());
                break 'read;
            }
            let message = &chunk["message"];
            if let Some(content) = message["content"].as_str().filter(|c| !c.is_empty()) {
                record.ttft.get_or_insert_with(|| started.elapsed());
                record.text.push_str(content);
                record.content_chunks += 1;
            }
            if let Some(thinking) = message["thinking"].as_str() {
                record.thinking.push_str(thinking);
            }
            if let Some(calls) = message["tool_calls"].as_array().filter(|c| !c.is_empty()) {
                record.ttft.get_or_insert_with(|| started.elapsed());
                record.tool_call_chunk_index.get_or_insert(record.chunks);
                record.tool_calls.extend(calls.iter().cloned());
            }
            if chunk["done"].as_bool() == Some(true) {
                record.final_chunk = Some(chunk);
                break 'read;
            }
            if let Stop::AfterContentChunks(limit) = stop {
                if record.content_chunks >= limit {
                    record.stopped_early = true;
                    break 'read;
                }
            }
        }
    }
    // Dropping `stream` here closes the connection: that is how a client cancels.
    drop(stream);
    record.wall = started.elapsed();
    Ok(record)
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn secs(duration: Option<Duration>) -> Value {
    duration.map_or(Value::Null, |d| json!((d.as_secs_f64() * 1000.0).round() / 1000.0))
}

/// tokens/s etc. from Ollama's own nanosecond counters in the final chunk.
fn rates(final_chunk: Option<&Value>) -> Value {
    let Some(f) = final_chunk else { return Value::Null };
    let per_s = |count: &str, ns: &str| match (f[count].as_f64(), f[ns].as_f64()) {
        (Some(c), Some(n)) if n > 0.0 => json!((c / (n / 1e9) * 100.0).round() / 100.0),
        _ => Value::Null,
    };
    let s = |key: &str| f[key].as_f64().map_or(Value::Null, |ns| json!((ns / 1e6).round() / 1000.0));
    json!({
        "eval_tokens": f["eval_count"],
        "gen_tok_s": per_s("eval_count", "eval_duration"),
        "prompt_tokens": f["prompt_eval_count"],
        "prompt_tok_s": per_s("prompt_eval_count", "prompt_eval_duration"),
        "load_s": s("load_duration"),
        "total_s": s("total_duration"),
    })
}

// ------------------------------------------------------------------------------------ memory

/// Peak RSS of all processes whose command line mentions ollama (server + model runner).
struct RssSampler {
    stop: Arc<AtomicBool>,
    peak_total_kb: Arc<AtomicU64>,
    peak_runner_kb: Arc<AtomicU64>,
    handle: std::thread::JoinHandle<()>,
}

impl RssSampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak_total_kb = Arc::new(AtomicU64::new(0));
        let peak_runner_kb = Arc::new(AtomicU64::new(0));
        let (s, t, r) = (stop.clone(), peak_total_kb.clone(), peak_runner_kb.clone());
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if let Ok(out) = std::process::Command::new("ps").args(["-axo", "rss=,command="]).output() {
                    let (mut total, mut runner) = (0_u64, 0_u64);
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let mut parts = line.trim().splitn(2, char::is_whitespace);
                        let (Some(rss), Some(cmd)) = (parts.next(), parts.next()) else { continue };
                        if !cmd.contains("ollama") || cmd.contains("ollama-spike") {
                            continue;
                        }
                        let rss: u64 = rss.parse().unwrap_or(0);
                        total += rss;
                        if cmd.contains(" runner") {
                            runner = runner.max(rss);
                        }
                    }
                    t.fetch_max(total, Ordering::Relaxed);
                    r.fetch_max(runner, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Self { stop, peak_total_kb, peak_runner_kb, handle }
    }

    fn finish(self) -> Value {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
        json!({
            "all_ollama_mb": self.peak_total_kb.load(Ordering::Relaxed) / 1024,
            "runner_mb": self.peak_runner_kb.load(Ordering::Relaxed) / 1024,
        })
    }
}

// ------------------------------------------------------------------------------------ checks

fn tool_def(name: &str) -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": name,
            "description": "Returns the current date and time as ISO-8601 for an IANA time zone.",
            "parameters": {
                "type": "object",
                "properties": {
                    "timezone": { "type": "string", "description": "IANA time zone name, e.g. Europe/Berlin" }
                },
                "additionalProperties": false
            }
        }
    }])
}

/// Looks like `Area/Location` (or deeper): letters, digits, `_`, `+`, `-`.
fn iana_like(value: &str) -> bool {
    let mut parts = value.split('/');
    let area_ok = parts.next().is_some_and(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphabetic() || c == '_'));
    let rest: Vec<&str> = parts.collect();
    area_ok
        && !rest.is_empty()
        && rest.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "_+-".contains(c)))
}

/// Well-formed = exactly one call, the exact tool name, object arguments, only known keys,
/// and (if present) an IANA-looking string time zone.
fn judge(record: &Record, expected: &str) -> Value {
    let mut verdict = json!({ "well_formed": false, "reason": null, "ttft_s": secs(record.ttft),
        "streamed_chunk_index": record.tool_call_chunk_index,
        "text_alongside": record.text.chars().take(120).collect::<String>() });
    let reason = (|| {
        if let Some(error) = &record.error {
            return Some(format!("error: {error}"));
        }
        if record.tool_calls.len() != 1 {
            return Some(format!("{} tool calls", record.tool_calls.len()));
        }
        let function = &record.tool_calls[0]["function"];
        verdict["name"] = function["name"].clone();
        verdict["arguments"] = function["arguments"].clone();
        let Some(args) = function["arguments"].as_object() else { return Some("arguments not an object".into()) };
        if function["name"].as_str() != Some(expected) {
            return Some("wrong name".into());
        }
        if args.keys().any(|k| k != "timezone") {
            return Some("unknown argument keys".into());
        }
        if let Some(tz) = args.get("timezone") {
            if !tz.as_str().is_some_and(iana_like) {
                return Some("timezone not IANA-like".into());
            }
        }
        None
    })();
    verdict["well_formed"] = json!(reason.is_none());
    verdict["reason"] = json!(reason);
    verdict["tokyo"] = json!(verdict["arguments"]["timezone"].as_str() == Some("Asia/Tokyo"));
    verdict
}

// ------------------------------------------------------------------------------------ steps

async fn get(http: &reqwest::Client, path: &str) -> Result<Value> {
    Ok(http.get(format!("{BASE}{path}")).send().await?.json().await?)
}

async fn unload(http: &reqwest::Client, model: &str) -> Result<()> {
    http.post(format!("{BASE}/api/generate"))
        .json(&json!({ "model": model, "keep_alive": 0 }))
        .send()
        .await?
        .bytes()
        .await?;
    for _ in 0..100 {
        let ps = get(http, "/api/ps").await?;
        let loaded = ps["models"].as_array().is_some_and(|m| m.iter().any(|x| x["name"] == model));
        if !loaded {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("model did not unload")
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 { f64::NAN } else if n % 2 == 1 { values[n / 2] } else { (values[n / 2 - 1] + values[n / 2]) / 2.0 }
}

#[tokio::main]
async fn main() -> Result<()> {
    let model = std::env::args().nth(1).context("usage: ollama-spike <model>")?;
    let http = reqwest::Client::builder()
        .no_proxy() // loopback traffic must never go through a proxy
        .build()?;
    let mut result = json!({
        "model": model,
        "num_ctx": NUM_CTX,
        "think": false,
        "ollama_version": get(&http, "/api/version").await?["version"],
        "started_unix_s": now_nanos() / 1_000_000_000,
    });
    let sampler = RssSampler::start();
    let user = |text: &str| json!([{ "role": "user", "content": text }]);
    let with_system = |text: &str| json!([{ "role": "system", "content": SYSTEM }, { "role": "user", "content": text }]);

    // 1. cold load
    unload(&http, &model).await?;
    let cold = stream_chat(&http, &chat_body(&model, user("Reply with one short sentence: hello."), None, json!({})), Stop::Never).await?;
    result["cold"] = json!({ "wall_s": secs(Some(cold.wall)), "ttft_s": secs(cold.ttft), "ollama": rates(cold.final_chunk.as_ref()) });
    result["ollama_ps"] = get(&http, "/api/ps").await?["models"].clone();
    println!("cold: {}", result["cold"]);

    // 2. warm streaming
    let mut warm = Vec::new();
    for _ in 0..WARM_RUNS {
        let body = chat_body(&model, user("Explain in about 120 words what a CPU cache is."), None, json!({ "num_predict": 200 }));
        let r = stream_chat(&http, &body, Stop::Never).await?;
        warm.push(json!({ "ttft_s": secs(r.ttft), "wall_s": secs(Some(r.wall)), "ollama": rates(r.final_chunk.as_ref()) }));
    }
    let pick = |key: &str| warm.iter().filter_map(|w| w[key].as_f64().or_else(|| w["ollama"][key].as_f64())).collect::<Vec<_>>();
    result["warm_summary"] = json!({
        "ttft_s_median": median(pick("ttft_s")),
        "ttft_s_max": pick("ttft_s").into_iter().fold(0.0, f64::max),
        "gen_tok_s_median": median(pick("gen_tok_s")),
        "gen_tok_s_min": pick("gen_tok_s").into_iter().fold(f64::INFINITY, f64::min),
        "prompt_tok_s_median": median(pick("prompt_tok_s")),
    });
    result["warm_runs"] = json!(warm);
    println!("warm: {}", result["warm_summary"]);

    // 3/4. tool calls: dotted name (the Loki contract name) vs underscore
    result["tool_trials"] = json!({});
    for name in ["time.now", "time_now"] {
        let mut trials = Vec::new();
        for _ in 0..TRIALS {
            let body = chat_body(&model, with_system(TIME_PROMPT), Some(tool_def(name)), json!({}));
            trials.push(judge(&stream_chat(&http, &body, Stop::Never).await?, name));
        }
        let ok = trials.iter().filter(|t| t["well_formed"] == true).count();
        let tokyo = trials.iter().filter(|t| t["tokyo"] == true).count();
        println!("tool {name}: {ok}/{TRIALS} well-formed, {tokyo} with Asia/Tokyo");
        result["tool_trials"][name] = json!({ "well_formed": ok, "of": TRIALS, "tokyo": tokyo, "trials": trials });
    }

    // 5. false positives
    let mut spurious = Vec::new();
    for prompt in NO_TOOL_PROMPTS {
        let body = chat_body(&model, with_system(prompt), Some(tool_def("time_now")), json!({}));
        let r = stream_chat(&http, &body, Stop::Never).await?;
        spurious.push(json!({ "prompt": prompt, "tool_calls": r.tool_calls.len(), "text": r.text.chars().take(80).collect::<String>() }));
    }
    let with_call = spurious.iter().filter(|s| s["tool_calls"].as_u64() > Some(0)).count();
    println!("false positives: {with_call}/{}", spurious.len());
    result["false_positives"] = json!({ "with_tool_call": with_call, "of": spurious.len(), "trials": spurious });

    // 6. round trip
    let first = stream_chat(&http, &chat_body(&model, with_system(TIME_PROMPT), Some(tool_def("time_now")), json!({})), Stop::Never).await?;
    let mut round_trip = json!({ "first_calls": first.tool_calls });
    if !first.tool_calls.is_empty() {
        let messages = json!([
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": TIME_PROMPT },
            { "role": "assistant", "content": first.text, "tool_calls": first.tool_calls },
            { "role": "tool", "tool_name": "time_now", "content": "{\"now\":\"2026-11-28T00:31:00+09:00\"}" }
        ]);
        let second = stream_chat(&http, &chat_body(&model, messages, Some(tool_def("time_now")), json!({})), Stop::Never).await?;
        round_trip["final_text"] = json!(second.text.chars().take(300).collect::<String>());
        round_trip["uses_tool_result"] = json!(second.text.contains("00:31") || second.text.contains("12:31"));
        round_trip["second_tool_calls"] = json!(second.tool_calls.len());
    }
    println!("round trip: {}", round_trip["final_text"]);
    result["round_trip"] = round_trip;

    // 7a. cancellation by dropping the stream after 20 content chunks
    let long = chat_body(&model, user("Count from 1 to 400, one number per line, nothing else."), None, json!({ "num_predict": 1500 }));
    let dropped = stream_chat(&http, &long, Stop::AfterContentChunks(20)).await?;
    let dropped_at_unix_ms = now_nanos() / 1_000_000;
    let follow = stream_chat(&http, &chat_body(&model, user("Reply with one word: ok"), None, json!({})), Stop::Never).await?;
    // 7b. cancellation through a token while the reader is waiting in next()
    let token = CancellationToken::new();
    let cancelled_at = Arc::new(AtomicU64::new(0));
    let (t, at) = (token.clone(), cancelled_at.clone());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        at.store(now_nanos(), Ordering::SeqCst);
        t.cancel();
    });
    let by_token = stream_chat(&http, &long, Stop::Token(token, cancelled_at)).await?;
    let follow_token = stream_chat(&http, &chat_body(&model, user("Reply with one word: ok"), None, json!({})), Stop::Never).await?;
    result["cancellation"] = json!({
        "drop": { "content_chunks_before_drop": dropped.content_chunks, "stopped_early": dropped.stopped_early,
                  "drop_after_s": secs(Some(dropped.wall)), "dropped_at_unix_ms": dropped_at_unix_ms,
                  "next_request_ttft_s": secs(follow.ttft), "next_request_wall_s": secs(Some(follow.wall)) },
        "token": { "content_chunks_before_cancel": by_token.content_chunks, "stopped_early": by_token.stopped_early,
                   "cancel_to_exit_ms": by_token.cancel_to_exit.map(|d| d.as_secs_f64() * 1000.0),
                   "next_request_ttft_s": secs(follow_token.ttft), "next_request_wall_s": secs(Some(follow_token.wall)) },
    });
    println!("cancellation: {}", result["cancellation"]);

    // 8. cloud refusal (server runs with OLLAMA_NO_CLOUD=1)
    let cloud = stream_chat(&http, &chat_body("gpt-oss:20b-cloud", user("hi"), None, json!({})), Stop::Never).await?;
    result["cloud_check"] = json!({ "status": cloud.status, "error": cloud.error, "got_text": !cloud.text.is_empty() });
    println!("cloud check: {}", result["cloud_check"]);

    result["peak_rss"] = sampler.finish();
    println!("peak rss: {}", result["peak_rss"]);
    std::fs::create_dir_all("output")?;
    let path = format!("output/{}.json", model.replace([':', '/'], "-"));
    std::fs::write(&path, serde_json::to_string_pretty(&result)?)?;
    println!("wrote {path}");
    Ok(())
}
