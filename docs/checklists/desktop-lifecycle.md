# Desktop lifecycle checklist

Run this on each desktop OS: the reference OS in P5-T10, and the others in P6-T05. Copy the table
into the handoff with results.

| # | Scenario | Expected | Result |
| --- | --- | --- | --- |
| 1 | Close the window while idle | The window hides; the tray icon remains; no mic in use | |
| 2 | Reopen from the tray | The same window and state; the conversation is intact | |
| 3 | Close the window during generation | Generation continues or stops (per setting); there's no crash | |
| 4 | Quit from the tray during generation | Work is cancelled, the mic is released, the process exits within 3 s | |
| 5 | Quit while recording | The mic indicator turns off immediately | |
| 6 | Launch a second instance | The first instance is focused; no second process | |
| 7 | Reboot with autostart **off** | Loki doesn't start | |
| 8 | Reboot with autostart **on** | Loki starts to the tray, not in front of the user | |
| 9 | Sleep and resume while idle | Idle state, the model reachable, reminders reconciled | |
| 10 | Sleep during recording | After resume: `Idle`, the mic released, a message shown | |
| 11 | Sleep during a tool run | The task is `Interrupted` or finished, never replayed | |
| 12 | Kill the process during a tool run, then restart | The task is shown as `Interrupted`; there's no automatic re-execution | |
| 13 | Revoke the mic permission in OS settings while running | A clear error, and a link to fix it | |
| 14 | Uninstall | The app is removed; the docs explain where user data and models remain and how to delete them | |
