async page => {
  const base = 'http://127.0.0.1:1434/tests/fixtures/settings.html?terminal-shell';
  await page.goto(base);
  await page.setViewportSize({ width: 1100, height: 820 });
  const select = page.getByRole('combobox', { name: 'Agent startup mode' });
  await select.waitFor();
  for (const mode of ['login', 'interactive', 'interactive_login', 'auto']) {
    await select.selectOption(mode);
    await page.waitForFunction(mode => document.querySelector('[data-testid="action"]').textContent === mode, mode);
    if (await select.inputValue() !== mode) throw new Error(`Mode did not save: ${mode}`);
  }
  await page.screenshot({ path: '/tmp/workman-agent-shell-settings.png' });
  await page.setViewportSize({ width: 380, height: 820 });
  const overflow = await page.locator('.terminal-section').evaluate(el => el.scrollWidth > el.clientWidth + 1);
  if (overflow) throw new Error('Terminal settings overflow at narrow width');
  await page.goto(`${base}&fail-save`);
  await select.selectOption('login');
  await page.getByText('Fixture save failed', { exact: true }).waitFor();
  if (await select.inputValue() !== 'auto') throw new Error('Failed save did not restore previous mode');
  await page.goto(`${base}&windows`);
  await page.getByRole('heading', { name: 'Terminal environment' }).waitFor();
  if (await select.count()) throw new Error('Unix startup options shown on Windows');
  return 'Startup modes save, failure recovery, narrow layout, and Windows control visibility passed';
}
