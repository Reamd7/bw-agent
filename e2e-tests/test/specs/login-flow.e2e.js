/**
 * Login E2E Test — Full unlock flow via native credential prompt.
 *
 * Flow:
 *   1. wdio.conf.js `before` hook collects master password via native dialog
 *      → stored in globalThis.__e2eAccounts (memory only, never on disk)
 *   2. Test enters password → clicks Unlock → waits for dashboard
 *
 * App auth stages (SolidJS state machine):
 *   "password" → "submitting" → "Success" (→ #/dashboard)
 *                                or "TwoFactorRequired" (→ 2FA stage)
 *                                or Error (→ "password" with attempt counter)
 *
 * App routing (SolidJS HashRouter):
 *   #/           → LoginPage (when locked)
 *   #/dashboard  → DashboardPage (protected, redirects to #/ when locked)
 */

describe('Login Flow', () => {
  // ── Constants ──────────────────────────────────────────────
  const SELECTORS = {
    // Login page
    passwordInput: 'input[placeholder="Master password"]',
    unlockBtn: 'button.btn-primary',
    settingsBtn: 'button[title="Settings"]',

    // Dashboard
    sidebarItem: 'button.sidebar-item',
  };

  // ── Helpers ────────────────────────────────────────────────

  /** Get the master password from the pre-collected credentials */
  function getMasterPassword() {
    const account = globalThis.__e2eAccounts?.[0];
    if (!account) {
      throw new Error('No credentials collected. Did the credential prompt run?');
    }
    return account.password;
  }

  /** Wait for an element to exist and return it */
  async function waitFor(selector, timeout = 15000) {
    const el = await $(selector);
    await el.waitForExist({ timeout });
    return el;
  }

  /** Enter password and click Unlock */
  async function unlockWithPassword(password) {
    const input = await waitFor(SELECTORS.passwordInput);
    await input.clearValue();
    await input.addValue(password);

    const btn = await $(SELECTORS.unlockBtn);
    await btn.click();
  }

  // ── Tests ──────────────────────────────────────────────────

  it('should display the login page with password input', async () => {
    const input = await waitFor(SELECTORS.passwordInput, 10000);
    expect(await input.isExisting()).toBe(true);

    const unlockBtn = await $(SELECTORS.unlockBtn);
    expect(await unlockBtn.getText()).toContain('Unlock');
  });

  it('should log in with the master password', async () => {
    const password = getMasterPassword();

    // Enter password and unlock
    await unlockWithPassword(password);

    // Wait for dashboard navigation — SolidJS uses window.location.hash
    await browser.waitUntil(
      async () => {
        const url = await browser.getUrl();
        return url.includes('#/dashboard');
      },
      {
        timeout: 30000,
        timeoutMsg: 'Expected to navigate to #/dashboard within 30s. Login may have failed or 2FA may be required.',
      },
    );

    // Verify dashboard sidebar is visible
    const sidebarItems = await $$(SELECTORS.sidebarItem);
    expect(sidebarItems.length).toBeGreaterThan(0);
  });

  it('should display the user email in the sidebar', async () => {
    // The sidebar shows the email from the config
    const emailSpan = await $('aside .text-xs.truncate');
    await emailSpan.waitForExist({ timeout: 5000 });
    const text = await emailSpan.getText();
    expect(text).toBeTruthy();
    expect(text).toContain('@');
  });

  it('should lock the vault and return to login page', async () => {
    // Find and click "Lock Vault" button
    const lockBtn = await $('button.sidebar-item=Lock Vault');
    await lockBtn.click();

    // Should navigate back to login page (#/)
    await browser.waitUntil(
      async () => {
        const url = await browser.getUrl();
        return url.includes('#/') && !url.includes('#/dashboard');
      },
      {
        timeout: 10000,
        timeoutMsg: 'Expected to navigate back to #/ after locking vault',
      },
    );

    // Login page should be visible again
    const passwordInput = await waitFor(SELECTORS.passwordInput, 5000);
    expect(await passwordInput.isExisting()).toBe(true);
  });
});
