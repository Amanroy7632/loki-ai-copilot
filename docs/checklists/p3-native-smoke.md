# P3 native smoke test (macOS, reference OS)

Run on a real Mac after changes to tools, policy or the macOS adapter. Record the result in STATUS.md.

```sh
./scripts/ollama-serve.sh                       # terminal 1
D=$(mktemp -d)                                  # throwaway data dir
loki --data-dir $D models use bonsai-2:4b
loki --data-dir $D permissions grant clock
loki --data-dir $D permissions grant system_info
loki --data-dir $D permissions grant app_launch
loki --data-dir $D chat
#   you> What time is it in Tokyo right now?   -> [tool time.now: succeeded], correct time
#   you> Open the Calculator app.               -> app.list, then the approval prompt for app.open
#        Approve? [y/N] n                       -> not launched, audit shows cancelled
#   you> Open the Calculator app.               -> approve with y: Calculator opens, "succeeded"
loki --data-dir $D audit list                   # three-plus rows, redacted summaries only
```

## Last run: 2026-11-27, MacBook Air M2 8 GB, macOS 27.0.1, Ollama 0.40.18, bonsai-2:4b

- [x] `permissions list`: clock/system_info/app_launch/file_read supported; network, microphone,
      notifications, developer_tools shown **unavailable** with the phase they arrive in.
- [x] `time.now` called by the model unprompted: "It is 2:07 AM on Saturday in Tokyo." (correct: 22:37 IST)
- [x] "Open the Calculator app": model called `app.list`, then `app.open {"app_id":"com.apple.calculator"}`;
      the approval preview showed tool, risk `LocalChange` and every argument.
- [x] Approved with `y`: Calculator launched; verified by its running process; audit
      `launched com.apple.calculator (verified)` in 206 ms.
- [x] Rejected path (`n`): covered by the automated CLI test `rejected_approval_at_the_prompt_launches_nothing`
      against the real macOS adapter (nothing launched, task `cancelled`).
- [x] Audit rows written for every call.
