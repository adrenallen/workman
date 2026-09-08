async page => {
  const assert = (ok, message) => { if (!ok) throw new Error(message); };
  const base = 'http://127.0.0.1:1432/tests/fixtures/settings.html';
  await page.goto(base);
  await page.setViewportSize({ width: 1100, height: 780 });
  await page.getByRole('heading', { name: 'Agent templates', exact: true }).waitFor();
  await page.getByRole('button', { name: 'Edit General Agent', exact: true }).click();
  await page.getByLabel('Name', { exact: true }).fill('Focused worker');
  await page.getByRole('button', { name: 'Save template', exact: true }).click();
  await page.getByText('Focused worker', { exact: true }).waitFor();
  await page.getByRole('tab', { name: 'Appearance', exact: true }).click();
  assert((await page.getByRole('tab', {name: 'Appearance', exact: true}).getAttribute('aria-selected')) === 'true', 'Appearance tab selected');
  await page.getByRole('tab', { name: 'Appearance', exact: true }).press('ArrowDown');
  assert((await page.getByRole('tab', {name: 'Terminal', exact: true}).getAttribute('aria-selected')) === 'true', 'Arrow navigation moves through sections');
  await page.getByRole('tab', { name: 'Templates', exact: true }).click();
  for (const theme of ['dark', 'light']) {
    for (const width of [380, 650, 760, 1100]) {
      await page.setViewportSize({ width, height: 780 });
      for (const scale of [0.9, 1, 1.1, 1.2]) {
        await page.evaluate(({theme, scale}) => {
          document.documentElement.dataset.theme = theme;
          document.documentElement.style.setProperty('--ui-font-scale', String(scale));
        }, {theme, scale});
        await page.waitForFunction(() => document.querySelector('.settings-panel').getBoundingClientRect().width <= innerWidth);
        const overflow = await page.locator('.section-panel').evaluate(el => ({width: el.clientWidth, scroll: el.scrollWidth}));
        assert(overflow.scroll <= overflow.width + 1, `Settings overflow at ${width}/${theme}/${scale}: ${JSON.stringify(overflow)}`);
      }
    }
  }
  await page.evaluate(() => document.documentElement.style.setProperty('--ui-font-scale', '1'));
  await page.setViewportSize({width: 380, height: 780});
  await page.getByRole('button', { name: 'Settings section', exact: true }).click();
  await page.getByRole('option', { name: 'Hotkeys', exact: true }).click();
  await page.getByRole('heading', { name: 'Hotkeys', exact: true }).waitFor();
  await page.getByRole('option', { name: 'Hotkeys', exact: true }).waitFor({state: 'hidden'});
  await page.screenshot({path: '/tmp/workman-retro-settings-narrow.png'});
  await page.goto(`${base}?welcome`);
  await page.getByRole('heading', {name: 'Let’s get to work.'}).waitFor();
  await page.getByRole('button', {name: 'Add a project', exact: true}).click();
  assert(await page.getByTestId('action').textContent() === 'add', 'Welcome project action');
  await page.getByRole('button', {name: 'Switch profiles', exact: true}).click();
  assert(await page.getByTestId('action').textContent() === 'profiles', 'Welcome profile action');
  for (const theme of ['dark', 'light']) {
    for (const width of [380, 650, 1100]) {
      await page.setViewportSize({width, height: 650});
      await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
      const overflow = await page.locator('.welcome').evaluate(el => el.scrollWidth > el.clientWidth + 1);
      assert(!overflow, `Welcome overflow at ${width}/${theme}`);
      assert(await page.getByRole('button', {name: 'Add a project', exact: true}).isVisible(), 'Welcome action remains visible');
    }
  }
  await page.screenshot({path: '/tmp/workman-retro-welcome-light.png'});
  return 'Settings navigation, template editing, 32 responsive/theme/type combinations, and welcome actions passed';
}
