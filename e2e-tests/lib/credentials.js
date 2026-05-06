import { execFileSync } from 'child_process';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const ps1Script = path.join(__dirname, 'credential-prompt.ps1');

/**
 * Detect if running in CI (no desktop available).
 */
function isCI() {
  return !!(process.env.CI || process.env.GITHUB_ACTIONS || process.env.TF_BUILD);
}

/**
 * Read credentials from environment variables (CI mode).
 * Pattern: E2E_ACCOUNT_{n}_EMAIL / E2E_ACCOUNT_{n}_PASSWORD
 *
 * @param {number} index - 1-based account index
 * @returns {{ email: string, password: string } | null}
 */
function fromEnv(index) {
  const email = process.env[`E2E_ACCOUNT_${index}_EMAIL`];
  const password = process.env[`E2E_ACCOUNT_${index}_PASSWORD`];
  if (email && password) return { email, password };
  return null;
}

/**
 * Show a native Windows dialog to collect email + password.
 * The values exist only in memory - never written to disk.
 *
 * @param {object} opts
 * @param {number} opts.accountIndex - 1-based account number for the title
 * @param {string} [opts.titleOverride] - Custom dialog title
 * @param {string} [opts.emailDefault] - Pre-fill email field
 * @returns {Promise<{ email: string, password: string }>}
 */
function promptNative({ accountIndex = 1, titleOverride, emailDefault = '' }) {
  return new Promise((resolve, reject) => {
    const title = titleOverride || `E2E Test - Account ${accountIndex}`;

    try {
      const stdout = execFileSync(
        'powershell',
        [
          '-ExecutionPolicy', 'Bypass',
          '-NonInteractive',
          '-File', ps1Script,
          '-Title', title,
          '-EmailDefault', emailDefault,
        ],
        {
          encoding: 'utf-8',
          timeout: 300000, // 5 min timeout — user might step away
          windowsHide: false, // Allow the dialog to show
        },
      );

      const creds = JSON.parse(stdout.trim());
      if (!creds.email || !creds.password) {
        throw new Error('Email or password was empty.');
      }
      resolve(creds);
    } catch (err) {
      if (err.status === 1) {
        reject(new Error('User cancelled the credential dialog.'));
      } else {
        reject(new Error(`Credential prompt failed: ${err.message}`));
      }
    }
  });
}

/**
 * Get credentials for an account.
 * - CI: reads from E2E_ACCOUNT_{n}_EMAIL / E2E_ACCOUNT_{n}_PASSWORD env vars
 * - Local: shows native Windows dialog
 *
 * @param {number} accountIndex - 1-based account number
 * @param {object} [opts] - Options passed to promptNative
 * @returns {Promise<{ email: string, password: string }>}
 */
export async function getCredential(accountIndex, opts = {}) {
  // CI: read from env vars
  if (isCI()) {
    const creds = fromEnv(accountIndex);
    if (!creds) {
      throw new Error(
        `CI mode: E2E_ACCOUNT_${accountIndex}_EMAIL and E2E_ACCOUNT_${accountIndex}_PASSWORD env vars are required.`,
      );
    }
    return creds;
  }

  // Local: show native dialog
  return promptNative({ accountIndex, ...opts });
}

/**
 * Collect multiple accounts interactively.
 * Keeps prompting until user cancels or env vars run out.
 *
 * @param {number} count - Number of accounts to collect
 * @returns {Promise<Array<{ email: string, password: string }>>}
 */
export async function getCredentials(count = 1) {
  const accounts = [];
  for (let i = 1; i <= count; i++) {
    // eslint-disable-next-line no-await-in-loop
    const creds = await getCredential(i);
    accounts.push(creds);
  }
  return accounts;
}
