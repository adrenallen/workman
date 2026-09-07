These fixtures mount the real scratchpad editor and recent-tab switcher with disposable
in-memory coordination data. They never connect to a Workman daemon.

Start Vite on port 1432, then run the interaction regression in fresh Chrome and WebKit
sessions with the Playwright CLI:

```sh
npm run dev -- --port 1432
playwright-cli -s=scratchpad-chrome open http://localhost:1432/tests/fixtures/scratchpad-editor.html --browser chrome
playwright-cli -s=scratchpad-chrome run-code --filename=tests/fixtures/scratchpad-interactions.js
playwright-cli -s=scratchpad-webkit open http://localhost:1432/tests/fixtures/scratchpad-editor.html --browser webkit
playwright-cli -s=scratchpad-webkit run-code --filename=tests/fixtures/scratchpad-interactions.js
```

Run `scratchpad-editor-sync.js` and `scratchpad-native-key-release.js` in separate fresh
sessions. They stub the native boundary to exercise file synchronization/conflicts and
modifier release without a DOM keyup. They do not prove native editor launch or OS key
delivery. Rust tests cover the real editor-file IO and containment checks.

The interaction fixture checks a document scrolled more than 10,000 pixels, exact cursor
restoration, unfinished edits, quick taps without an overlay flash, held/repeated shortcuts, both arrows, Escape, literal code
copying, in-place edits, and typing a new fence. Clipboard writes are captured without
changing the system clipboard. It saves a screenshot to `/tmp/workman-scratchpad-code-ui.png`.

For actual macOS testing, build the app and launch a disposable copy with
`scripts/native-visual-qa.sh`. Test ordinary text clicks and Cmd+` in that native window;
browser-only tests do not exercise the native menu accelerator. Stop only the disposable
app and its isolated daemon when finished.

For process-resume focus coverage, start Vite on port 1433 and run
`process-resume-focus.js` in a fresh browser session. It checks terminal input,
sidebar and overview rows, running/starting processes, editors, dialogs, menus,
and removed processes against the real focus resolver, without starting processes.
