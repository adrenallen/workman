async page => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('http://127.0.0.1:1438/tests/fixtures/command-review.html');
  await page.getByRole('button', { name: 'Cancel', exact: true }).waitFor();
  await page.screenshot({ path: '/tmp/workman-trust-dark.png' });
  await page.evaluate(() => document.documentElement.dataset.theme = 'light');
  await page.waitForFunction(() => getComputedStyle(document.querySelector('[role=dialog]')).backgroundColor === 'rgb(255, 255, 255)');
  await page.screenshot({ path: '/tmp/workman-trust-light.png' });
  await page.setViewportSize({ width: 390, height: 700 });
  const dialog = page.getByRole('dialog');
  const box = await dialog.boundingBox();
  if (!box || box.x < 0 || box.x + box.width > 390) throw new Error('Trust dialog overflows');
  if (!(await page.getByRole('button', { name: 'Trust and run', exact: true }).isVisible())) throw new Error('Approval hidden');
  await page.screenshot({ path: '/tmp/workman-trust-narrow.png' });
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Choose branch', exact: true }).click();
  const ref = page.getByRole('combobox');
  if (await ref.inputValue() !== 'origin/main') throw new Error('Default ref changed');
  await ref.fill('origin/dev');
  await ref.press('Enter');
  if (await ref.inputValue() !== 'origin/dev') throw new Error('Exact ref switched to HEAD');
  await page.getByRole('textbox', { name: 'Branch name', exact: true }).fill('feature/fixture');
  await page.getByRole('button', { name: 'Create worktree', exact: true }).click();
  const result = JSON.parse(await page.locator('output').innerText());
  if (result.fromRef !== 'origin/dev') throw new Error(`Wrong submitted base: ${JSON.stringify(result)}`);
  console.log('Trust dark/light/narrow, Escape dismissal, exact origin/dev selection and submission passed.');
}
