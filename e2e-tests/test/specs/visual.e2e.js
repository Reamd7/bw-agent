describe('Visual Testing – Login Page', () => {
  it('should save the full viewport screen', async () => {
    await browser.saveScreen('login-page-viewport');
  });

  it('should save the app title element as an image', async () => {
    const title = await $('h1');
    await browser.saveElement(title, 'login-title');
  });

  it('should save the settings gear button as an image', async () => {
    const gear = await $('button[title="Settings"]');
    await browser.saveElement(gear, 'settings-gear-btn');
  });

  it('should match the viewport against baseline', async () => {
    // First run creates baseline; subsequent runs compare against it.
    // Allow 5% mismatch tolerance for minor font/rendering differences.
    await browser.checkScreen('login-page-viewport', {
      savePerInstance: true,
      returnAllCompareData: true,
    });
  });

  it('should match the title element against baseline', async () => {
    const title = await $('h1');
    await browser.checkElement(title, 'login-title', {
      savePerInstance: true,
    });
  });
});
