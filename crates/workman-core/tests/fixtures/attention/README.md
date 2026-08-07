# Attention fixtures

These rendered screens come from isolated real Workman PTY sessions on macOS. ANSI styling, account details, and unrelated welcome content are removed, while status, composer, footer, and permission text used by the attention classifier are preserved.

- `claude_working.txt`: Haiku answering a no-tools prompt while its animated status and interrupt footer were visible.
- `claude_resting_with_draft.txt`: the completed response, timing line, and an unsubmitted draft matching the todo 255 observation.
- `claude_permission_dialog.txt`: manual mode requesting approval for a harmless `touch` command; the command was denied and never executed.
- `codex_working.txt`: Codex 0.146.1 during a turn, captured in Todo 422's isolated `com.workman.todo422` session.
- `codex_resting.txt`: the same Codex session at its resting composer after the Shift+Enter acceptance probe.
- `plain_terminal_working.txt`: ordinary shell output before its prompt returned.
- `plain_terminal_prompt.txt`: the same shell after its prompt returned.

## Recording another adapter

1. Launch only a per-todo isolated Workman identity and fresh `/tmp` data/config.
2. Capture the server-rendered viewport at three boundaries: active work, the final resting composer, and any explicit input/permission dialog. Do not copy secrets, repository content, or account identifiers.
3. Remove only ANSI styling and unrelated welcome/history text. Keep exact prompt glyphs, spinner wording, interrupt/footer wording, and the last eight non-empty lines because adapters use their relative order.
4. Add the fixture to `notification_pipeline_tests.rs` and replay it on both sides of the five-second confirmation boundary.
5. Record tool/version, date, and isolated todo provenance in this file.

The Claude 2.1.221 fixtures were recorded on 2026-08-04. The Codex 0.146.1 fixture was recorded during Todo 422's isolated acceptance on 2026-08-06; the plain-shell pair is normalized from the isolated raw-mode PTY probes retained from Todos 416/422.
