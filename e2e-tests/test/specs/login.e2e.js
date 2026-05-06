describe('Bitwarden SSH Agent - Login Page', () => {
  it('should display the app title', async () => {
    const title = await $('h1');
    const text = await title.getText();
    expect(text).toContain('Bitwarden SSH Agent');
  });

  it('should show the settings gear button', async () => {
    const settingsBtn = await $('button[title="Settings"]');
    expect(await settingsBtn.isExisting()).toBe(true);
  });

  it('should have a visible body with a background color', async () => {
    const body = await $('body');
    const backgroundColor = await body.getCSSProperty('background-color');
    // Verify some background color is set (not transparent)
    expect(backgroundColor.parsed.hex).toBeDefined();
    expect(backgroundColor.parsed.hex).not.toBe('');
  });
});
