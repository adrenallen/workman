# Attention fixtures

The Claude screens in this directory were captured from real interactive Claude Code 2.1.221 PTYs on macOS on 2026-08-04. ANSI styling and unrelated welcome content were removed, while the rendered status, composer, footer, and permission text used by the attention classifier were preserved.

- `claude_working.txt`: Haiku answering a no-tools prompt while its animated status and interrupt footer were visible.
- `claude_resting_with_draft.txt`: the completed response, timing line, and an unsubmitted draft matching the todo 255 observation.
- `claude_permission_dialog.txt`: manual mode requesting approval for a harmless `touch` command; the command was denied and never executed.
