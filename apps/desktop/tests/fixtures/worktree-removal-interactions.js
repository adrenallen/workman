async page => {
  const base = 'http://127.0.0.1:1434/tests/fixtures/worktree-removal.html';
  const assert = (value, message) => { if (!value) throw new Error(message); };
  await page.setViewportSize({ width: 900, height: 800 });
  for (const scenario of ['clean', 'dirty', 'commits', 'dependent']) {
    await page.goto(`${base}?scenario=${scenario}`);
    const checkbox = page.getByRole('checkbox', { name: 'Also delete this project from my computer' });
    await checkbox.waitFor();
    assert(!(await checkbox.isChecked()), `${scenario}: deletion should be opt-in`);
    assert(await page.getByRole('button', { name: 'Remove from Workman', exact: true }).isVisible(), 'unregister default');
    assert(!(await page.getByText('Local work at risk', { exact: true }).isVisible()), 'no warning when keeping files');
    await checkbox.click();
    const text = await page.getByRole('alertdialog').innerText();
    assert(!/not merged into|ignored local paths|No upstream/.test(text), `${scenario}: obsolete warning`);
    if (scenario === 'clean') {
      assert(!text.includes('Local work at risk'), 'clean checkout should have no extra warning');
      assert(text.includes('The branch is kept'), 'explain retained branch');
    } else {
      assert(text.includes('Local work at risk'), `${scenario}: missing loss warning`);
    }
    if (scenario === 'dirty') {
      assert(text.includes('2 uncommitted files · 1 untracked'), 'uncommitted file summary');
      assert(text.includes('src/app.ts') && text.includes('notes.txt'), 'exact file paths');
      assert(!text.includes('local commit'), 'no commit warning for a retained branch');
    }
    if (scenario === 'commits') {
      assert(text.includes('1 local commit at risk') && text.includes('Save local implementation'), 'local commit details');
      assert(text.includes('This worktree has no branch') && !text.includes('The branch is kept'), 'detached checkout copy');
    }
    if (scenario === 'dependent') assert(text.includes('/tmp/dependent-worktree'), 'dependent checkout safeguard');
    const button = page.getByRole('button', { name: scenario === 'clean' ? 'Delete project' : 'Delete anyway', exact: true });
    assert(await button.isVisible(), 'delete confirmation visible');
    await button.click();
    const result = JSON.parse(await page.locator('output').innerText());
    assert(result.deleteFromDisk && result.forceDirty === (scenario !== 'clean'), 'correct confirmation payload');
  }
  await page.goto(`${base}?scenario=dirty`);
  await page.getByRole('button', { name: 'Remove from Workman', exact: true }).click();
  assert(await page.locator('output').innerText() === '{"deleteFromDisk":false,"forceDirty":false}', 'keep-files path remains unforced');
  console.log('Removal clean/dirty/local-commit/dependent cases and confirmation payloads passed. No files deleted.');
}
