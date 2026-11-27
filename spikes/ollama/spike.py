#!/usr/bin/env python3
"""P0-T03 Ollama spike: streaming, tool calling, cancellation, latency and memory.

Throwaway (spikes/README.md). Standard library only. Talks to a local Ollama on loopback.

    python3 -I spike.py --model qwen3:4b --out output/qwen3-4b.json

Measures, for one model:
  1. cold load: unload the model, then time the first request (wall clock + Ollama's load_duration)
  2. warm streaming x5: time to first token, generation tokens/s, prompt tokens/s
  3. peak RAM: RSS of all `ollama` processes, sampled every 100 ms, plus /api/ps `size`
  4. tool calls: 10 trials with the tool named `time.now`, 10 with `time_now`
  5. false positives: 10 prompts that need no tool, tools still offered
  6. tool round trip: feed a tool result back and get the final answer
  7. cancellation: drop the connection mid-stream, then time the next request
"""

import argparse
import http.client
import json
import re
import statistics
import subprocess
import threading
import time

HOST, PORT = "127.0.0.1", 11434
TRIALS = 10
WARM_RUNS = 5
OPTIONS = {"num_ctx": 4096}  # bounded context, so memory numbers are comparable on 8 GB
SYSTEM = "You are Loki, a local assistant. Use a tool only when it is needed to answer."
TIME_PROMPT = "What time is it right now in Tokyo?"
NO_TOOL_PROMPTS = [
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
]


def tool_def(name):
    return {
        "type": "function",
        "function": {
            "name": name,
            "description": "Returns the current date and time as ISO-8601 for an IANA time zone.",
            "parameters": {
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "IANA time zone name, e.g. Europe/Berlin",
                    }
                },
                "additionalProperties": False,
            },
        },
    }


# ---------------------------------------------------------------- HTTP helpers


def post_json(path, body, timeout=600):
    conn = http.client.HTTPConnection(HOST, PORT, timeout=timeout)
    conn.request("POST", path, json.dumps(body), {"Content-Type": "application/json"})
    resp = conn.getresponse()
    data = resp.read()
    conn.close()
    return resp.status, json.loads(data) if data else None


def get_json(path):
    conn = http.client.HTTPConnection(HOST, PORT, timeout=30)
    conn.request("GET", path)
    resp = conn.getresponse()
    data = json.loads(resp.read())
    conn.close()
    return data


def stream_chat(body, stop_after_chunks=None):
    """Streams /api/chat. Returns a record with timings, text, tool calls and final metrics.

    With stop_after_chunks, closes the socket after that many content chunks (cancellation test).
    """
    body = dict(body, stream=True)
    started = time.perf_counter()
    conn = http.client.HTTPConnection(HOST, PORT, timeout=600)
    conn.request("POST", "/api/chat", json.dumps(body), {"Content-Type": "application/json"})
    resp = conn.getresponse()
    record = {"status": resp.status, "ttft_s": None, "text": "", "thinking": "",
              "tool_calls": [], "tool_call_chunk_index": None, "chunks": 0,
              "final": None, "cancelled": False, "error": None}
    if resp.status != 200:
        record["error"] = resp.read().decode("utf-8", "replace")[:300]
        conn.close()
        return record
    content_chunks = 0
    while True:
        line = resp.readline()
        if not line:
            break
        chunk = json.loads(line)
        record["chunks"] += 1
        if "error" in chunk:
            record["error"] = chunk["error"]
            break
        message = chunk.get("message", {})
        if message.get("content"):
            if record["ttft_s"] is None:
                record["ttft_s"] = time.perf_counter() - started
            record["text"] += message["content"]
            content_chunks += 1
        if message.get("thinking"):
            record["thinking"] += message["thinking"]
        if message.get("tool_calls"):
            if record["ttft_s"] is None:
                record["ttft_s"] = time.perf_counter() - started
            if record["tool_call_chunk_index"] is None:
                record["tool_call_chunk_index"] = record["chunks"]
            record["tool_calls"].extend(message["tool_calls"])
        if chunk.get("done"):
            record["final"] = {k: v for k, v in chunk.items() if k != "message"}
            break
        if stop_after_chunks is not None and content_chunks >= stop_after_chunks:
            record["cancelled"] = True
            break
    record["wall_s"] = time.perf_counter() - started
    resp.close()
    conn.close()  # dropping the socket is how a client cancels
    return record


def rates(final):
    """tokens/s from Ollama's own nanosecond counters."""
    def per_s(count, ns):
        return round(count / (ns / 1e9), 2) if count and ns else None
    return {
        "eval_tokens": final.get("eval_count"),
        "gen_tok_s": per_s(final.get("eval_count"), final.get("eval_duration")),
        "prompt_tokens": final.get("prompt_eval_count"),
        "prompt_tok_s": per_s(final.get("prompt_eval_count"), final.get("prompt_eval_duration")),
        "load_s": round(final.get("load_duration", 0) / 1e9, 3),
        "total_s": round(final.get("total_duration", 0) / 1e9, 3),
    }


# ---------------------------------------------------------------- memory sampler


class RssSampler(threading.Thread):
    """Peak resident memory of every process whose command line mentions ollama."""

    def __init__(self):
        super().__init__(daemon=True)
        self.peak_total_kb = 0
        self.peak_runner_kb = 0
        self._stop = threading.Event()

    def run(self):
        while not self._stop.is_set():
            out = subprocess.run(["ps", "-axo", "rss=,command="], capture_output=True, text=True).stdout
            total = runner = 0
            for line in out.splitlines():
                parts = line.strip().split(None, 1)
                if len(parts) == 2 and "ollama" in parts[1] and "spike.py" not in parts[1]:
                    rss = int(parts[0])
                    total += rss
                    if " runner" in parts[1]:
                        runner = max(runner, rss)
            self.peak_total_kb = max(self.peak_total_kb, total)
            self.peak_runner_kb = max(self.peak_runner_kb, runner)
            self._stop.wait(0.1)

    def stop(self):
        self._stop.set()


# ---------------------------------------------------------------- checks


IANA = re.compile(r"^[A-Za-z_]+(/[A-Za-z0-9_+\-]+)+$")


def judge_tool_call(record, expected_name):
    """A well-formed call: exactly one call, the exact tool name, object arguments, only known
    keys, and a string time zone that looks like an IANA name."""
    calls = record["tool_calls"]
    verdict = {"well_formed": False, "reason": None, "name": None, "arguments": None}
    if record["error"]:
        verdict["reason"] = "error: " + str(record["error"])[:120]
        return verdict
    if len(calls) != 1:
        verdict["reason"] = f"{len(calls)} tool calls"
        return verdict
    fn = calls[0].get("function", {})
    verdict["name"], verdict["arguments"] = fn.get("name"), fn.get("arguments")
    args = fn.get("arguments")
    if fn.get("name") != expected_name:
        verdict["reason"] = "wrong name"
    elif not isinstance(args, dict):
        verdict["reason"] = "arguments not an object"
    elif set(args) - {"timezone"}:
        verdict["reason"] = "unknown argument keys"
    elif "timezone" in args and not (isinstance(args["timezone"], str) and IANA.match(args["timezone"])):
        verdict["reason"] = "timezone not IANA-like"
    else:
        verdict["well_formed"] = True
    verdict["tokyo"] = isinstance(args, dict) and args.get("timezone") == "Asia/Tokyo"
    return verdict


def unload(model):
    post_json("/api/generate", {"model": model, "keep_alive": 0})
    for _ in range(100):
        if not any(m["name"] == model for m in get_json("/api/ps").get("models", [])):
            return
        time.sleep(0.1)


def chat_body(model, messages, tools=None, **extra):
    body = {"model": model, "messages": messages, "think": False, "options": OPTIONS}
    if tools:
        body["tools"] = tools
    body.update(extra)
    return body


# ---------------------------------------------------------------- main


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    model = args.model
    result = {"model": model, "options": OPTIONS, "think": False,
              "ollama_version": get_json("/api/version")["version"]}
    sampler = RssSampler()
    sampler.start()

    # 1. cold load
    unload(model)
    cold = stream_chat(chat_body(model, [{"role": "user", "content": "Reply with one short sentence: hello."}]))
    result["cold"] = {"wall_s": round(cold["wall_s"], 3), "ttft_s": round(cold["ttft_s"] or -1, 3), **rates(cold["final"])}
    ps = get_json("/api/ps").get("models", [])
    result["ollama_ps"] = [{"name": m["name"], "size_bytes": m.get("size"), "size_vram_bytes": m.get("size_vram"),
                            "context_length": m.get("context_length")} for m in ps]
    print("cold:", result["cold"])

    # 2. warm streaming
    warm = []
    for _ in range(WARM_RUNS):
        rec = stream_chat(chat_body(model, [{"role": "user", "content": "Explain in about 120 words what a CPU cache is."}],
                                    options=dict(OPTIONS, num_predict=200)))
        warm.append({"ttft_s": round(rec["ttft_s"], 3), "wall_s": round(rec["wall_s"], 3), **rates(rec["final"])})
    result["warm_runs"] = warm
    result["warm_summary"] = {
        "ttft_s_median": round(statistics.median(r["ttft_s"] for r in warm), 3),
        "ttft_s_max": max(r["ttft_s"] for r in warm),
        "gen_tok_s_median": statistics.median(r["gen_tok_s"] for r in warm),
        "gen_tok_s_min": min(r["gen_tok_s"] for r in warm),
        "prompt_tok_s_median": statistics.median(r["prompt_tok_s"] for r in warm if r["prompt_tok_s"]),
    }
    print("warm:", result["warm_summary"])

    # 3/4. tool calls with a dotted name (the Loki contract name) and an underscore name
    result["tool_trials"] = {}
    for name in ["time.now", "time_now"]:
        trials = []
        for _ in range(TRIALS):
            rec = stream_chat(chat_body(model, [{"role": "system", "content": SYSTEM},
                                                {"role": "user", "content": TIME_PROMPT}], tools=[tool_def(name)]))
            verdict = judge_tool_call(rec, name)
            verdict["ttft_s"] = round(rec["ttft_s"] or -1, 3)
            verdict["text_alongside"] = rec["text"][:120]
            verdict["streamed_chunk_index"] = rec["tool_call_chunk_index"]
            trials.append(verdict)
        ok = sum(t["well_formed"] for t in trials)
        result["tool_trials"][name] = {"well_formed": ok, "of": TRIALS,
                                       "tokyo": sum(bool(t.get("tokyo")) for t in trials), "trials": trials}
        print(f"tool {name}: {ok}/{TRIALS} well-formed")

    # 5. false positives: no tool needed
    spurious = []
    for prompt in NO_TOOL_PROMPTS:
        rec = stream_chat(chat_body(model, [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}],
                                    tools=[tool_def("time_now")]))
        spurious.append({"prompt": prompt, "tool_calls": len(rec["tool_calls"]), "text": rec["text"][:80]})
    result["false_positives"] = {"with_tool_call": sum(1 for s in spurious if s["tool_calls"]),
                                 "of": len(spurious), "trials": spurious}
    print("false positives:", result["false_positives"]["with_tool_call"], "/", len(spurious))

    # 6. round trip: model calls the tool, we answer as the tool, model writes the final reply
    first = stream_chat(chat_body(model, [{"role": "system", "content": SYSTEM},
                                          {"role": "user", "content": TIME_PROMPT}], tools=[tool_def("time_now")]))
    round_trip = {"first_call": first["tool_calls"][:1]}
    if first["tool_calls"]:
        messages = [{"role": "system", "content": SYSTEM}, {"role": "user", "content": TIME_PROMPT},
                    {"role": "assistant", "content": first["text"], "tool_calls": first["tool_calls"]},
                    {"role": "tool", "tool_name": "time_now", "content": json.dumps({"now": "2026-11-28T00:31:00+09:00"})}]
        second = stream_chat(chat_body(model, messages, tools=[tool_def("time_now")]))
        round_trip["final_text"] = second["text"][:300]
        round_trip["mentions_time"] = bool(re.search(r"00:31|12:31|31", second["text"]))
        round_trip["second_tool_calls"] = len(second["tool_calls"])
    result["round_trip"] = round_trip
    print("round trip:", round_trip.get("final_text", "(no first call)")[:100])

    # 7. cancellation: drop the socket after 20 chunks of a long generation
    long_body = chat_body(model, [{"role": "user", "content": "Count from 1 to 400, one number per line, nothing else."}],
                          options=dict(OPTIONS, num_predict=1500))
    cancel_started = time.time()
    rec = stream_chat(long_body, stop_after_chunks=20)
    dropped_at = time.time()
    follow = stream_chat(chat_body(model, [{"role": "user", "content": "Reply with one word: ok"}]))
    result["cancellation"] = {
        "chunks_before_drop": rec["chunks"],
        "drop_after_s": round(rec["wall_s"], 3),
        "next_request_ttft_s": round(follow["ttft_s"] or -1, 3),
        "next_request_wall_s": round(follow["wall_s"], 3),
        "dropped_at_unix": dropped_at,
        "started_at_unix": cancel_started,
    }
    print("cancellation:", result["cancellation"])

    sampler.stop()
    sampler.join()
    result["peak_rss"] = {"all_ollama_mb": round(sampler.peak_total_kb / 1024), "runner_mb": round(sampler.peak_runner_kb / 1024)}
    print("peak rss:", result["peak_rss"])
    with open(args.out, "w") as handle:
        json.dump(result, handle, indent=2)


if __name__ == "__main__":
    main()
