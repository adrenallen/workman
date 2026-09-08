// Run with playwright-cli run-code --filename=... against the isolated Vite fixture.
async page => {
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  await page.goto('http://localhost:1432/tests/fixtures/scratchpad-editor.html');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.waitForSelector('.cm-content');

  // Inspect the actual painted pixels, not just CSS: CodeMirror's drawn
  // selection used to have the right color but was hidden by code backgrounds.
  async function expectBlueSelection(locator, label) {
    const clip = await locator.boundingBox();
    const png = await page.screenshot({ clip, scale: 'css' });
    const result = await page.evaluate(async ({ png, selector }) => {
      const image = new Image();
      image.src = `data:image/png;base64,${png}`;
      await image.decode();
      const canvas = document.createElement('canvas');
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext('2d');
      context.drawImage(image, 0, 0);
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      const style = getComputedStyle(document.querySelector(selector), '::selection');
      const rgb = style.backgroundColor.match(/[\d.]+/g).map(Number);
      let bluePixels = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        // WebKit blends native selection backgrounds with the surface at 80%.
        if (pixels[i + 2] > pixels[i] + 35 &&
          rgb.slice(0, 3).every((channel, j) => Math.abs(channel - pixels[i + j]) < 25)) bluePixels++;
      }
      return { bluePixels, rgb };
    }, { png: png.toString('base64'), selector: '.cm-content' });
    assert(result.rgb.length === 3 && result.rgb[2] > result.rgb[0] + 30, `${label}: selection must be opaque blue`);
    assert(result.bluePixels > 80, `${label}: selection is not visibly painted (${result.bluePixels} blue pixels)`);
  }

  for (const theme of ['dark', 'light']) {
    await page.getByRole('button', { name: 'Section 1', exact: true }).click();
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    await page.waitForFunction(theme => getComputedStyle(document.querySelector('.cm-code-line')).backgroundColor ===
      (theme === 'dark' ? 'rgb(22, 25, 30)' : 'rgb(255, 255, 255)'), theme);
    const code = page.locator('.cm-code-line').filter({ hasText: 'const section = 1;' }).first();
    await code.scrollIntoViewIfNeeded();
    const top = await page.locator('.document-viewport').evaluate(el => el.scrollTop);
    // Drag across literal code using real pointer events.
    const points = await code.evaluate(el => {
      const range = document.createRange();
      range.selectNodeContents(el);
      const r = range.getBoundingClientRect();
      return { start: r.left + 1, end: r.right - 1, y: r.top + r.height / 2 };
    });
    await page.mouse.move(points.start, points.y);
    await page.mouse.down();
    await page.mouse.move(points.end, points.y, { steps: 12 });
    await page.mouse.up();
    await page.waitForFunction(() => getSelection()?.toString() === 'const section = 1;');
    await expectBlueSelection(code, `${theme} code drag`);
    const copied = await code.evaluate(el => {
      const clipboardData = new DataTransfer();
      el.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, clipboardData }));
      return clipboardData.getData('text/plain');
    });
    assert(copied === 'const section = 1;', `${theme}: copy did not preserve the selected code`);
    assert(Math.abs(await page.locator('.document-viewport').evaluate(el => el.scrollTop) - top) < 2, 'Selecting code scrolled the scratchpad');
    await page.screenshot({ path: `/tmp/workman-selection-${theme}.png` });

    const prose = page.locator('.cm-line').filter({ hasText: 'Paragraph 1.1:' }).first();
    await prose.click({ clickCount: 3 });
    await expectBlueSelection(prose, `${theme} prose`);
    const inlineLine = page.locator('.cm-line').filter({ hasText: 'Text after the code block with' }).first();
    await inlineLine.click({ clickCount: 3 });
    await expectBlueSelection(inlineLine.locator('.cm-live-inline-code'), `${theme} inline code`);

    const title = page.getByRole('textbox', { name: 'Scratchpad title', exact: true });
    await title.click();
    await title.selectText();
    await expectBlueSelection(title, `${theme} title`);

    // Reveal a distant comment through the document's outline (the editor virtualizes).
    await page.getByRole('button', { name: 'Section 28', exact: true }).click();
    const comment = page.locator('.cm-comment-highlight').first();
    await comment.scrollIntoViewIfNeeded();
    await comment.click({ clickCount: 3 });
    await expectBlueSelection(comment, `${theme} comment anchor`);
  }
  return { passed: ['painted blue selection in both themes: code, prose, inline code, title, comment anchor', 'mouse drag', 'selected code copy', 'stable scroll'] };
}
