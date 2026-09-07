// Run against the isolated scratchpad-editor.html fixture with playwright-cli run-code --filename=...
async page => {
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('http://localhost:1432/tests/fixtures/scratchpad-editor.html');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('.cm-content');
  await page.waitForTimeout(250);
  // Exercise the shared copy boundary without changing the user's system clipboard.
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true,
    value: { writeText: async text => { window.__copiedCode = text; } } }));
  const readPosition = () => page.evaluate(() => ({
    top: document.querySelector('.document-viewport').scrollTop,
    text: getSelection()?.anchorNode?.textContent, offset: getSelection()?.anchorOffset
  }));
  await page.locator('.document-viewport').evaluate(el => { el.scrollTop = 14000; });
  await page.waitForTimeout(300);
  const beforeClick = await readPosition();
  assert(beforeClick.top > 10000, 'Regression must exercise a deeply scrolled document');
  const point = await page.evaluate(() => {
    const line = [...document.querySelectorAll('.cm-line')].find(el => {
      const r = el.getBoundingClientRect(); return r.top > 230 && r.bottom < 400 && el.textContent.startsWith('Paragraph');
    });
    const r = line.getBoundingClientRect(); return { x: r.left + 90, y: r.top + r.height / 2 };
  });
  await page.mouse.click(point.x, point.y);
  const before = await readPosition();
  assert(Math.abs(before.top - beforeClick.top) < 2, `Clicking ordinary text moved the scratchpad: ${beforeClick.top} → ${before.top}`);
  assert(before.text.startsWith('Paragraph'), 'Click did not place the cursor in the clicked paragraph');

  const modifier = await page.evaluate(() => /Mac/.test(navigator.platform) ? 'Meta' : 'Control');
  await page.evaluate(() => {
    window.__switcherFlashed = false;
    window.__switcherObserver = new MutationObserver(records => {
      for (const record of records) for (const node of record.addedNodes) {
        if (node instanceof Element && (node.matches('.recent-backdrop') || node.querySelector('.recent-backdrop'))) window.__switcherFlashed = true;
      }
    });
    window.__switcherObserver.observe(document.body, { childList: true, subtree: true });
  });
  await page.keyboard.down(modifier);
  await page.keyboard.press('Backquote');
  await page.keyboard.up(modifier);
  await page.getByText('Another pane', { exact: true }).waitFor();
  assert(!(await page.evaluate(() => window.__switcherFlashed)), 'A quick tap flashed the recent-tab list');
  await page.keyboard.down(modifier);
  await page.keyboard.press('Backquote');
  await page.keyboard.up(modifier);
  await page.waitForSelector('.cm-content');
  await page.waitForTimeout(200);
  assert(!(await page.evaluate(() => window.__switcherFlashed)), 'A quick return tap flashed the recent-tab list');
  await page.evaluate(() => window.__switcherObserver.disconnect());
  await page.keyboard.down(modifier);
  await page.keyboard.press('Backquote');
  assert(await page.getByRole('option', { selected: true }).innerText() === 'Overview\nFixture', 'First press should select the previous tab');
  await page.keyboard.press('ArrowDown');
  assert((await page.getByRole('option', { selected: true }).innerText()).includes('Editor regression'), 'Arrow keys did not cycle');
  await page.keyboard.press('Backquote');
  assert(await page.getByRole('option', { selected: true }).innerText() === 'Overview\nFixture', 'Repeated backquote did not cycle');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.up(modifier);
  await page.getByText('Another pane', { exact: true }).waitFor();
  assert(await page.getByRole('listbox', { name: 'Recent tabs' }).count() === 0, 'Switcher stayed open after modifier release');
  await page.keyboard.down(modifier);
  await page.keyboard.press('Backquote');
  await page.keyboard.up(modifier);
  await page.waitForSelector('.cm-content');
  await page.waitForTimeout(300);
  const returned = await readPosition();
  assert(Math.abs(returned.top - before.top) < 2, 'Tab return lost scroll position');
  assert(returned.text === before.text && returned.offset === before.offset, 'Tab return lost cursor position');

  await page.keyboard.down(modifier);
  await page.keyboard.press('Backquote');
  await page.keyboard.press('Escape');
  await page.keyboard.up(modifier);
  assert(await page.locator('.cm-content').count() === 1, 'Escape should cancel without navigating');
  await page.keyboard.insertText('UNSAVED_EDIT');
  await page.getByRole('button', { name: 'Switch pane' }).click();
  await page.getByRole('button', { name: 'Switch pane' }).click();
  await page.waitForTimeout(300);
  assert((await page.locator('.cm-content').innerText()).includes('UNSAVED_EDIT'), 'Switching before autosave lost the draft');

  const header = await page.evaluate(() => {
    const node = [...document.querySelectorAll('.cm-code-header')].find(el => {
      const r = el.getBoundingClientRect(); return r.top > 80 && r.bottom < innerHeight - 40;
    });
    if (!node) throw new Error('Expected a visible code header for copy test');
    const lines = [];
    for (let line = node.nextElementSibling; line && !line.classList.contains('cm-code-footer'); line = line.nextElementSibling) lines.push(line.textContent);
    node.dataset.testCode = 'chosen';
    return lines.join('\n') + '\n';
  });
  const copyTop = (await readPosition()).top;
  await page.locator('[data-test-code="chosen"]').getByRole('button', { name: 'Copy code block' }).click();
  await page.getByRole('button', { name: 'Code copied' }).waitFor();
  assert(await page.evaluate(() => window.__copiedCode) === header, 'Copy changed code whitespace or included fence markers');
  assert(header.includes('**literal** [text]'), 'Code content was interpreted as Markdown');
  assert(Math.abs((await readPosition()).top - copyTop) < 2, 'Copy scrolled the document');
  await page.locator('[data-test-code="chosen"]').getByRole('button', { name: 'Edit code block' }).click();
  assert(Math.abs((await readPosition()).top - copyTop) < 2, 'Edit button scrolled the document');
  await page.keyboard.insertText('// EDITED_IN_PLACE');
  assert((await page.locator('.cm-content').innerText()).includes('// EDITED_IN_PLACE'), 'Code could not be edited in place');
  await page.screenshot({ path: '/tmp/workman-scratchpad-code-ui.png' });
  await page.keyboard.press(modifier === 'Meta' ? 'Meta+ArrowDown' : 'Control+End');
  await page.keyboard.press('Enter');
  await page.keyboard.type('```js');
  await page.keyboard.press('Enter');
  await page.keyboard.type('const typedFence = 1;');
  await page.keyboard.press('Enter');
  await page.keyboard.type('```');
  assert((await page.locator('.cm-code-line').allTextContents()).some(line => line.includes('const typedFence = 1;')), 'Typing a fence did not render an editable code block');
  assert(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { passed: ['plain-text click', 'quick tap without list flash', 'hold/repeat/arrows/release switcher', 'cancel switcher', 'scroll and cursor restore', 'unsaved draft restore', 'literal code copy', 'edit code in place', 'type a new fence'], before, returned };
}
