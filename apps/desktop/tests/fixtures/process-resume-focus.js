async page => {
  await page.goto('http://localhost:1433/tests/fixtures/scratchpad-editor.html');
  return page.evaluate(async () => {
    const { focusedResumeProcess } = await import('/src/lib/processResume.ts');
    const processes = [
      { id: 1, kind: 'terminal', status: 'exited' },
      { id: 2, kind: 'agent', status: 'running' },
      { id: 3, kind: 'agent', status: 'stopped' },
      { id: 4, kind: 'command', status: 'crashed' },
      { id: 5, kind: 'terminal', status: 'starting' }
    ];
    const host = document.body.appendChild(document.createElement('div'));
    const passed = [];
    const check = (label, html, selector, expected) => {
      host.innerHTML = html;
      const target = host.querySelector(selector);
      target.focus();
      if (document.activeElement !== target) throw new Error(`${label}: fixture did not focus target`);
      const actual = focusedResumeProcess(document.activeElement, processes)?.id ?? null;
      if (actual !== expected) throw new Error(`${label}: expected ${expected}, got ${actual}`);
      passed.push(label);
    };
    try {
      check('stopped terminal input', '<section data-resume-process-id="1"><div class="xterm"><textarea readonly></textarea></div></section>', 'textarea', 1);
      check('focused agent row', '<button data-context-kind="agent" data-context-id="3">Agent</button>', 'button', 3);
      check('crashed command overview', '<article data-resume-process-id="4"><button>Command</button></article>', 'button', 4);
      check('running shell keeps its keys', '<section data-resume-process-id="2"><div class="xterm"><textarea></textarea></div></section>', 'textarea', null);
      check('starting process is not resumed again', '<section data-resume-process-id="5"><button>Starting</button></section>', 'button', null);
      check('terminal search keeps its keys', '<section data-resume-process-id="1"><input></section>', 'input', null);
      check('scratchpad focus has no process', '<div contenteditable="true">Notes</div>', 'div', null);
      check('dialog blocks resume', '<div role="dialog"><button data-resume-process-id="1">Dialog</button></div>', 'button', null);
      check('menu blocks resume', '<div role="menu"><button data-context-kind="agent" data-context-id="3">Menu</button></div>', 'button', null);
      check('removed process cannot resume', '<button data-context-kind="terminal" data-context-id="99">Removed</button>', 'button', null);
      check('todo with matching ID cannot resume', '<button data-context-kind="todo" data-context-id="1">Todo</button>', 'button', null);
      check('unrelated focus ignores selected process', '<button>Settings</button>', 'button', null);
      return { passed };
    } finally { host.remove(); }
  });
}
