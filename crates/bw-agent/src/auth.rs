use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn ensure_unlocked(
    state: Arc<Mutex<State>>,
    client: &bw_core::api::Client,
) -> anyhow::Result<()> {
    let is_unlocked = state.lock().await.is_unlocked();
    if is_unlocked {
        state.lock().await.touch();
        return Ok(());
    }

    let password_str = tokio::task::spawn_blocking(bw_ui::prompt_master_password)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Password prompt cancelled by user"))?;

    let mut password_vec = bw_core::locked::Vec::new();
    password_vec.extend(password_str.bytes());
    let password = bw_core::locked::Password::new(password_vec);

    let login_context = {
        let state = state.lock().await;
        if state.access_token.is_none() {
            LoginContext::FirstLogin {
                email: state
                    .email
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Email not configured"))?,
            }
        } else {
            LoginContext::Reunlock {
                email: state
                    .email
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Email not configured"))?,
                access_token: state
                    .access_token
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing access token"))?,
                kdf: state.kdf.ok_or_else(|| anyhow::anyhow!("Missing KDF type"))?,
                iterations: state
                    .iterations
                    .ok_or_else(|| anyhow::anyhow!("Missing KDF iterations"))?,
                memory: state.memory,
                parallelism: state.parallelism,
                protected_key: state
                    .protected_key
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing protected key"))?,
                protected_private_key: state
                    .protected_private_key
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing protected private key"))?,
                protected_org_keys: state.protected_org_keys.clone(),
            }
        }
    };

    match login_context {
        LoginContext::FirstLogin { email } => {
            let session = bw_core::api::full_login(client, &email, &password).await?;
            let sync_data = bw_core::api::sync_vault(client, &session.access_token).await?;
            let (keys, org_keys) = bw_core::api::unlock_vault(
                &email,
                &password,
                session.kdf,
                session.iterations,
                session.memory,
                session.parallelism,
                &session.protected_key,
                &sync_data.protected_private_key,
                &sync_data.org_keys,
            )?;

            let mut state = state.lock().await;
            state.access_token = Some(session.access_token);
            state.refresh_token = Some(session.refresh_token);
            state.email = Some(email);
            state.kdf = Some(session.kdf);
            state.iterations = Some(session.iterations);
            state.memory = session.memory;
            state.parallelism = session.parallelism;
            state.protected_key = Some(session.protected_key);
            state.protected_private_key = Some(sync_data.protected_private_key);
            state.protected_org_keys = sync_data.org_keys;
            state.entries = sync_data.entries;
            state.set_unlocked(keys, org_keys);
        }
        LoginContext::Reunlock {
            email,
            access_token,
            kdf,
            iterations,
            memory,
            parallelism,
            protected_key,
            protected_private_key,
            protected_org_keys,
        } => {
            let (keys, org_keys) = bw_core::api::unlock_vault(
                &email,
                &password,
                kdf,
                iterations,
                memory,
                parallelism,
                &protected_key,
                &protected_private_key,
                &protected_org_keys,
            )?;

            {
                let mut state = state.lock().await;
                state.set_unlocked(keys, org_keys);
            }

            if let Ok(sync_data) = bw_core::api::sync_vault(client, &access_token).await {
                let mut state = state.lock().await;
                state.protected_private_key = Some(sync_data.protected_private_key);
                state.protected_org_keys = sync_data.org_keys;
                state.entries = sync_data.entries;
            }
        }
    }

    Ok(())
}

pub fn decrypt_cipher(
    state: &State,
    cipherstring: &str,
    entry_key: Option<&str>,
    org_id: Option<&str>,
) -> anyhow::Result<String> {
    let keys = state
        .key(org_id)
        .ok_or_else(|| anyhow::anyhow!("No decryption keys available"))?;

    let entry_keys = if let Some(entry_key) = entry_key {
        let cipher = bw_core::cipherstring::CipherString::new(entry_key)?;
        Some(bw_core::locked::Keys::new(
            cipher.decrypt_locked_symmetric(keys)?,
        ))
    } else {
        None
    };

    let cipher = bw_core::cipherstring::CipherString::new(cipherstring)?;
    let plaintext = cipher.decrypt_symmetric(keys, entry_keys.as_ref())?;
    Ok(String::from_utf8(plaintext)?)
}

enum LoginContext {
    FirstLogin {
        email: String,
    },
    Reunlock {
        email: String,
        access_token: String,
        kdf: bw_core::api::KdfType,
        iterations: u32,
        memory: Option<u32>,
        parallelism: Option<u32>,
        protected_key: String,
        protected_private_key: String,
        protected_org_keys: std::collections::HashMap<String, String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_cipher_returns_error_when_locked() {
        let state = State::new(std::time::Duration::from_secs(900));
        let result = decrypt_cipher(&state, "2.fake|data|mac", None, None);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("locked state should fail")
                .to_string()
                .contains("No decryption keys")
        );
    }
}
