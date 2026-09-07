// Regression for an accelerator that never delivers keyup to the webview.
// The native modifier read is stubbed here; this does not prove OS event behavior.
async page => {
  await page.addInitScript(() => {
    window.isTauri = true;
    window.__primaryHeld = true;
    window.__TAURI_INTERNALS__ = { invoke: async command => {
      if (command === 'desktop_primary_modifier_pressed') return window.__primaryHeld;
      if (command === 'shell_detect_editors') return [];
      throw new Error(`Unexpected native command: ${command}`);
    }};
  });
  await page.goto('http://localhost:1432/tests/fixtures/scratchpad-editor.html');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('.cm-content');
  await page.getByRole('button', { name: 'Native shortcut', exact: true }).click();
  await page.getByRole('listbox', { name: 'Recent tabs' }).waitFor();
  await page.waitForTimeout(180);
  if (await page.getByRole('listbox', { name: 'Recent tabs' }).count() !== 1) throw new Error('List must stay open while Command is held');
  await page.evaluate(() => { window.__primaryHeld = false; });
  await page.getByText('Another pane', { exact: true }).waitFor();
  if (await page.getByRole('listbox', { name: 'Recent tabs' }).count() !== 0) throw new Error('Native release did not commit without DOM keyup');
  await page.getByRole('button', { name: 'Native shortcut', exact: true }).click();
  await page.waitForSelector('.cm-content');
  return { passed: ['hold via native menu', 'release without DOM keyup', 'quick native tap'] };
}
