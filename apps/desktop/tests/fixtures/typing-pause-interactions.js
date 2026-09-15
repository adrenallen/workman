async page => {
  const assert = (ok, message) => { if (!ok) throw new Error(message); };
  const base = 'http://127.0.0.1:1432/tests/fixtures/settings.html?terminal-shell';
  await page.goto(base);
  const toggle = page.getByRole('switch', { name: 'Pause automatic messages while typing' });
  const seconds = page.getByRole('spinbutton', { name: 'Typing idle delay in seconds' });
  const row = page.locator('.setting-row').filter({ has: toggle });
  const apply = row.getByRole('button', { name: 'Apply' });
  await toggle.waitFor();
  assert(await toggle.isChecked(), 'Enabled by default');
  assert(await seconds.inputValue() === '10', 'Default delay is 10 seconds');
  await seconds.fill('25');
  await apply.click();
  await page.waitForFunction(() => document.querySelector('[data-testid="action"]').textContent === '{"enabled":true,"delay_ms":25000}');
  await toggle.click();
  await page.waitForFunction(() => document.querySelector('[data-testid="action"]').textContent === '{"enabled":false,"delay_ms":25000}');
  assert(await seconds.isDisabled(), 'Disabled toggle disables the duration field');
  await toggle.click();
  await page.waitForFunction(() => document.querySelector('[data-testid="action"]').textContent === '{"enabled":true,"delay_ms":25000}');
  assert(await seconds.inputValue() === '25', 'Chosen duration survives toggling');
  for (const invalid of ['0', '3601', '1.5', '']) {
    await seconds.fill(invalid);
    await apply.click();
    await row.getByRole('status').waitFor();
    assert((await row.getByRole('status').textContent()).includes('1 to 3600'), 'Invalid duration rejected');
  }
  await seconds.fill('1');
  await seconds.press('Enter');
  await page.waitForFunction(() => document.querySelector('[data-testid="action"]').textContent === '{"enabled":true,"delay_ms":1000}');
  for (const width of [380, 760, 1100]) {
    await page.setViewportSize({ width, height: 900 });
    assert(await row.evaluate(el => el.scrollWidth <= el.clientWidth + 1), `Controls fit at ${width}px`);
  }
  await page.screenshot({ path: '/tmp/workman-typing-pause-settings.png' });
  await page.goto(`${base}&fail-save`);
  await toggle.click();
  await row.getByRole('status').waitFor();
  assert(await toggle.isChecked(), 'Failed save restores toggle');
  assert(await seconds.isEnabled(), 'Failed save keeps the existing preference');
  return 'Typing pause: defaults, duration, off/on, validation, responsive layout, and save failure passed';
}
