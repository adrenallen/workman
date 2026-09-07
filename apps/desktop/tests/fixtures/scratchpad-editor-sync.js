// Browser integration with a stub native file/editor boundary; native file IO is tested in Rust.
async page => {
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  await page.addInitScript(() => {
    window.isTauri = true;
    window.__editorFiles = {};
    window.__opened = [];
    window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
      if (command === 'shell_detect_editors') return [{ id: 'vscode', label: 'VS Code', bundle_path: '/fixture/Code.app' }];
      if (command === 'scratchpad_editor_create') {
        const path = `/fixture/notes-${Object.keys(window.__editorFiles).length}.md`;
        window.__editorFiles[path] = args.markdown;
        return path;
      }
      if (command === 'scratchpad_editor_read') {
        if (!(args.path in window.__editorFiles)) throw new Error('Editor file no longer exists');
        return window.__editorFiles[args.path];
      }
      if (command === 'shell_open_with') { window.__opened.push(args); return; }
      throw new Error(`Unexpected native command: ${command}`);
    }};
  });
  await page.goto('http://localhost:1432/tests/fixtures/scratchpad-editor.html');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.getByRole('button', { name: 'Open in VS Code', exact: true }).click();
  await page.waitForFunction(() => window.__opened.length === 1);
  assert(await page.evaluate(() => window.__opened[0].opener.id) === 'vscode', 'Configured editor was not used');
  await page.evaluate(() => { window.__editorFiles[window.__opened[0].path] = '# From editor\n\nSaved from the editor.'; });
  await page.waitForFunction(() => document.querySelector('.cm-content')?.textContent.includes('Saved from the editor.'));
  await page.waitForFunction(() => document.querySelector('.save-state')?.textContent.includes('Saved · rev 2'));
  await page.getByRole('button', { name: 'Open in VS Code', exact: true }).click();
  assert(await page.evaluate(() => Object.keys(window.__editorFiles).length) === 1, 'Unchanged session should reuse its file');

  await page.getByRole('button', { name: 'Agent edit', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.cm-content')?.textContent.includes('Agent addition.'));
  await page.evaluate(() => { window.__editorFiles[window.__opened[0].path] = '# Editor conflict\n\nA different editor revision.'; });
  await page.getByText('This scratchpad changed in more than one place.').waitFor();
  await page.getByRole('button', { name: 'Use theirs', exact: true }).click();
  assert((await page.locator('.cm-content').innerText()).includes('Agent addition.'), 'Conflict resolution lost the agent revision');
  assert(await page.evaluate(() => Object.values(localStorage).some(value => value.includes('A different editor revision.'))), 'Editor conflict draft was not retained');

  await page.getByRole('button', { name: 'Pause saves', exact: true }).click();
  await page.locator('.cm-content').click();
  await page.keyboard.insertText('LOCAL_UNSAVED');
  await page.evaluate(() => { window.__editorFiles[window.__opened[0].path] = '# Pending editor\n\nEditor changes while typing.'; });
  await page.getByRole('button', { name: 'Review editor changes', exact: true }).waitFor();
  assert((await page.locator('.cm-content').innerText()).includes('LOCAL_UNSAVED'), 'Background editor sync overwrote an unsaved Workman draft');
  await page.getByRole('button', { name: 'Review editor changes', exact: true }).click();
  await page.getByText('This scratchpad changed in more than one place.').waitFor();
  assert(await page.evaluate(() => Object.values(localStorage).some(value => value.includes('LOCAL_UNSAVED'))), 'Local draft was not preserved before reviewing editor changes');
  return { passed: ['configured editor handoff', 'external save auto-import', 'reuse editor file', 'agent conflict', 'preserve local edit during sync', 'recover both drafts'] };
}
