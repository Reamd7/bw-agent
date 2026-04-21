use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn ensure_unlocked<U: crate::UiCallback>(
    state: Arc<Mutex<State>>,
    client: &bw_core::api::Client,
    ui: &U,
) -> anyhow::Result<()> {
    let is_unlocked = state.lock().await.is_unlocked();
    if is_unlocked {
        state.lock().await.touch();
        return Ok(());
    }

    let email = state
        .lock()
        .await
        .email
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Email not configured"))?;

    let mut error: Option<String> = None;
    for _attempt in 0..3 {
        let password_str = ui
            .request_password(&email, error.as_deref())
            .await
            .ok_or_else(|| anyhow::anyhow!("Password prompt cancelled by user"))?;

        let mut password_vec = bw_core::locked::Vec::new();
        password_vec.extend(password_str.bytes());
        let password = bw_core::locked::Password::new(password_vec);

        match try_login(state.clone(), client, &email, &password, ui).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                error = Some(e.to_string());
            }
        }
    }

    anyhow::bail!("Failed to unlock after 3 attempts");
}

async fn try_login<U: crate::UiCallback>(
    state: Arc<Mutex<State>>,
    client: &bw_core::api::Client,
    email: &str,
    password: &bw_core::locked::Password,
    ui: &U,
) -> anyhow::Result<()> {
    let login_context = {
        let state = state.lock().await;
        if state.access_token.is_none() {
            LoginContext::FirstLogin
        } else {
            LoginContext::Reunlock {
                access_token: state
                    .access_token
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing access token"))?,
                kdf: state
                    .kdf
                    .ok_or_else(|| anyhow::anyhow!("Missing KDF type"))?,
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
        LoginContext::FirstLogin => {
            let session = match bw_core::api::full_login(client, email, password).await {
                Ok(session) => session,
                Err(bw_core::error::Error::TwoFactorRequired { providers, .. }) => {
                    let provider_ids: Vec<u8> = providers.iter().map(|p| *p as u8).collect();
                    let (provider_type, code) = ui
                        .request_two_factor(&provider_ids)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("Two-factor prompt cancelled"))?;

                    let (kdf, iterations, memory, parallelism) = client
                        .prelogin(email)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;
                    let identity = bw_core::identity::Identity::new(
                        email,
                        password,
                        kdf,
                        iterations,
                        memory,
                        parallelism,
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;

                    let two_factor_provider =
                        bw_core::api::TwoFactorProviderType::try_from(u64::from(provider_type))
                            .map_err(|e| anyhow::anyhow!(e))?;

                    let device_id = uuid::Uuid::new_v4().to_string();
                    let (access_token, refresh_token, protected_key) = client
                        .login(
                            email,
                            &device_id,
                            &identity.master_password_hash,
                            Some(&code),
                            Some(two_factor_provider),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;

                    bw_core::api::LoginSession {
                        access_token,
                        refresh_token,
                        kdf,
                        iterations,
                        memory,
                        parallelism,
                        protected_key,
                        email: email.to_string(),
                        identity,
                    }
                }
                Err(e) => return Err(anyhow::anyhow!(e)),
            };

            let sync_data = bw_core::api::sync_vault(client, &session.access_token)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            let (keys, org_keys) = bw_core::api::unlock_vault(
                email,
                password,
                session.kdf,
                session.iterations,
                session.memory,
                session.parallelism,
                &session.protected_key,
                &sync_data.protected_private_key,
                &sync_data.org_keys,
            )
            .map_err(|e| anyhow::anyhow!(e))?;

            let mut state = state.lock().await;
            state.access_token = Some(session.access_token);
            state.refresh_token = Some(session.refresh_token);
            state.email = Some(email.to_string());
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
                email,
                password,
                kdf,
                iterations,
                memory,
                parallelism,
                &protected_key,
                &protected_private_key,
                &protected_org_keys,
            )
            .map_err(|e| anyhow::anyhow!(e))?;

            {
                let mut state = state.lock().await;
                state.set_unlocked(keys, org_keys);
            }

            // Try to refresh the access token if we have a refresh_token
            let refresh_token = state.lock().await.refresh_token.clone();
            let fresh_token = match refresh_token {
                Some(rt) => match client.exchange_refresh_token(&rt).await {
                    Ok(new_token) => {
                        state.lock().await.access_token = Some(new_token.clone());
                        log::info!("Token refreshed successfully during re-unlock");
                        new_token
                    }
                    Err(e) => {
                        log::warn!(
                            "Token refresh failed during re-unlock: {e}, trying existing token"
                        );
                        access_token.clone()
                    }
                },
                None => {
                    log::debug!("No refresh token available, using existing access token");
                    access_token.clone()
                }
            };

            match bw_core::api::sync_vault(client, &fresh_token).await {
                Ok(sync_data) => {
                    let mut state = state.lock().await;
                    state.protected_private_key = Some(sync_data.protected_private_key);
                    state.protected_org_keys = sync_data.org_keys;
                    state.entries = sync_data.entries;
                    log::debug!("Vault sync succeeded during re-unlock");
                }
                Err(e) => {
                    log::warn!("Vault sync failed during re-unlock: {e}. Local keys remain valid.");
                }
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
    FirstLogin,
    Reunlock {
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
        let state = State::new(Some(std::time::Duration::from_secs(900)));
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
