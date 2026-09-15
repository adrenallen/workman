async page => {
  const assert = (ok, message) => { if (!ok) throw new Error(message); };
  await page.goto('http://127.0.0.1:1432/tests/fixtures/settings.html?notifications');
  const mode = page.getByRole('combobox', { name: 'Notification mode' });
  const input = page.getByRole('switch', { name: /^Agent needs input/ });
  const toasts = page.locator('.done-toast');
  const count = async expected => {
    await page.waitForFunction(n => document.querySelectorAll('.done-toast').length === n, expected, { timeout: 5000 });
  };
  await mode.waitFor();
  assert(await mode.isEnabled(), 'Scope remains configurable when computer notifications are off');
  assert(await input.isEnabled(), 'Input alerts remain configurable inside Workman');
  await count(2);
  assert(!(await toasts.allTextContents()).some(text => text.includes('Child')), 'Parent-only mode hides child completion and input toasts');
  await mode.selectOption('all');
  await count(4);
  await input.click();
  await count(2);
  assert(!(await toasts.allTextContents()).some(text => text.includes('needs input')), 'Input toggle filters existing toasts immediately');
  await mode.selectOption('top_level');
  await count(1);
  assert((await toasts.textContent()).includes('Parent finished'), 'Only parent completion remains');
  await mode.selectOption('project_ready');
  await count(0);
  await mode.selectOption('all');
  await count(2);
  await page.reload();
  await count(2);
  return 'In-app toasts respect parent/all/project-ready modes and input preferences, including with computer alerts off';
}
