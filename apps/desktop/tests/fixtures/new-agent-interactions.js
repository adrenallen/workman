// Disposable browser fixture; no agent, daemon, microphone, or system clipboard is used.
async page => {
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.setViewportSize({ width: 660, height: 760 });
  await page.goto('http://localhost:1432/tests/fixtures/new-agent.html');
  await page.waitForFunction(() => document.activeElement?.classList.contains('prompt-textarea'));
  assert(await page.locator('.draft-viewport').evaluate(el => el.scrollTop) === 0, 'Opening a draft scrolled past the starting-point chooser');
  await page.setViewportSize({ width: 1000, height: 1000 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const templateTab = page.getByRole('tab', { name: /Use a template/ });
  const modelTab = page.getByRole('tab', { name: /Choose a model/ });
  const prompt = () => page.locator('.prompt-textarea');
  const draft = async () => JSON.parse(await page.getByTestId('draft').textContent());
  const choose = name => page.locator('.launch-choice:visible').filter({ hasText: name }).click();
  assert(await templateTab.getAttribute('aria-selected') === 'true', 'Persisted template did not select its starting path');
  assert(await page.locator('.launch-choice:visible').count() === 2, 'Models compete with templates in the same list');
  assert(await page.getByRole('button', { name: /Model settings/ }).getAttribute('aria-expanded') === 'false', 'Settings must start collapsed');
  await page.screenshot({ path: '/tmp/workman-new-agent-dark.png' });

  await prompt().fill('Investigate this issue.');
  await page.getByRole('button', { name: 'Attach fixture', exact: true }).click();
  await choose('General Agent');
  assert((await draft()).templateId === 2 && (await draft()).agentToolId === 1, 'Template did not select its configured agent');
  await page.getByRole('button', { name: /Template instructions/ }).click();
  assert(await page.getByLabel('Template instructions preview').innerText() === 'Investigate the task, implement a focused solution, and verify the behavior.', 'Template preview changed');
  await page.getByRole('button', { name: /Runs with/ }).click();
  await page.locator('.override-choice').filter({ hasText: 'Claude' }).click();
  assert((await draft()).templateId === 2 && (await draft()).agentToolId === 2, 'Override discarded the template');
  await page.getByRole('button', { name: /Model settings/ }).click();
  await page.getByLabel('Name optional', { exact: true }).fill('Review worker');
  await page.getByLabel('Model optional override', { exact: true }).fill('sonnet');
  await page.getByLabel('Other launch args optional', { exact: true }).fill('--verbose');
  await page.getByRole('button', { name: 'Create agent', exact: true }).click();
  const submitted = JSON.parse(await page.getByTestId('submission').textContent());
  assert(submitted.agent_template_id === 2 && submitted.agent_tool_id === 2 && submitted.model === 'sonnet', 'Wrong template/agent/model payload');
  assert(submitted.prompt === 'Investigate this issue. [Image #1]' && submitted.attachments[0] === '/tmp/agent-fixture.png' && submitted.extra_args.includes('--verbose'), 'Prompt, images or launch arguments were lost');

  await modelTab.click();
  assert((await draft()).templateId === null && (await draft()).agentToolId === 2, 'Choosing a model did not remove template instructions');
  assert((await draft()).prompt === submitted.prompt && (await draft()).attachments.length === 1, 'Switching paths lost draft content');
  assert(await page.getByRole('button', { name: /Template instructions/ }).count() === 0, 'Template controls remain in the model path');
  await choose('Codex');
  assert((await draft()).agentToolId === 1 && (await draft()).model === '', 'Changing models retained an incompatible override');
  await page.getByRole('button', { name: /Model settings/ }).click();
  await page.screenshot({ path: '/tmp/workman-new-agent-models.png' });

  // The tab primitive owns arrow navigation and activates the matching launch path.
  await modelTab.focus();
  await page.keyboard.press('ArrowLeft');
  await page.waitForFunction(() => document.querySelector('[role="tab"][aria-selected="true"]')?.textContent.includes('Use a template'));
  assert((await draft()).templateId === 1, 'Keyboard tab switching did not update the launch choice');
  await page.getByRole('button', { name: 'Toggle busy', exact: true }).click();
  assert(await templateTab.isDisabled() && await modelTab.isDisabled() && await prompt().isDisabled(), 'Busy state allowed editing');
  await page.getByRole('button', { name: 'Toggle busy', exact: true }).click();

  await page.getByRole('button', { name: /Prompt history/ }).click();
  await page.locator('.history-entry > summary').click();
  await page.getByRole('button', { name: 'Use in new draft', exact: true }).click();
  assert(await modelTab.getAttribute('aria-selected') === 'true' && await prompt().inputValue() === 'Review the release and summarize the changes.', 'History restoration selected the wrong path or prompt');
  assert(await page.getByRole('dialog', { name: 'Prompt history' }).count() === 0, 'History stayed open after restoration');
  await page.getByRole('button', { name: /Prompt history/ }).click();
  await page.keyboard.press('Escape');
  assert((await page.locator(':focus').innerText()).includes('Prompt history'), 'Closing history did not return focus');

  for (const theme of ['light', 'dark']) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    for (const width of [1000, 540, 360]) {
      await page.setViewportSize({ width, height: 960 });
      for (const scale of [0.9, 1, 1.1, 1.2]) {
        await page.evaluate(scale => document.documentElement.style.setProperty('--ui-scale', String(scale)), scale);
        const overflow = await page.locator('.draft-shell').evaluate(el => el.scrollWidth > el.clientWidth + 1);
        assert(!overflow, `${theme}/${width}/${scale}: draft overflows horizontally`);
      }
      await page.evaluate(() => document.documentElement.style.setProperty('--ui-scale', '1'));
      await page.locator('.draft-viewport').evaluate(el => { el.scrollTop = 0; });
      await page.screenshot({ path: `/tmp/workman-new-agent-${theme}-${width}.png` });
      await page.getByRole('button', { name: 'Create agent', exact: true }).scrollIntoViewIfNeeded();
      assert(await page.getByRole('button', { name: 'Create agent', exact: true }).isVisible(), 'Create is unreachable in a narrow pane');
    }
  }
  await page.getByRole('button', { name: 'Toggle templates', exact: true }).click();
  assert(await templateTab.isDisabled(), 'An unavailable template path remains enabled');
  await page.getByRole('button', { name: 'Toggle tools', exact: true }).click();
  assert(await page.getByRole('button', { name: 'Create agent', exact: true }).isDisabled(), 'Missing tool can still launch');
  assert(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { passed: ['initial prompt focus without scrolling', 'exclusive starting paths', 'template preview and agent override', 'launch payload', 'prompt/attachment preservation', 'keyboard navigation', 'busy state', 'history restore and focus', 'light/dark and narrow/scaled layout', 'unavailable choices'] };
}
