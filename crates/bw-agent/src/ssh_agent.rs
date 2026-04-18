use crate::access_log::AccessLog;
use crate::approval::ApprovalQueue;
use crate::auth;
use crate::state::State;
use signature::Signer as _;
use std::sync::Arc;
use tokio::sync::Mutex;

const SSH_AGENT_RSA_SHA2_256: u32 = 2;
const SSH_AGENT_RSA_SHA2_512: u32 = 4;

pub struct SshAgentHandler<U: crate::UiCallback> {
    state: Arc<Mutex<State>>,
    client: bw_core::api::Client,
    ui: Arc<U>,
    approval_queue: Arc<ApprovalQueue>,
    access_log: Arc<AccessLog>,
    /// PID of the connected client. Set per-session in `new_session`.
    client_pid: u32,
}

impl<U: crate::UiCallback> Clone for SshAgentHandler<U> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            client: self.client.clone(),
            ui: Arc::clone(&self.ui),
            approval_queue: Arc::clone(&self.approval_queue),
            access_log: Arc::clone(&self.access_log),
            client_pid: self.client_pid,
        }
    }
}

impl<U: crate::UiCallback> SshAgentHandler<U> {
    pub fn new(
        state: Arc<Mutex<State>>,
        client: bw_core::api::Client,
        ui: Arc<U>,
        approval_queue: Arc<ApprovalQueue>,
        access_log: Arc<AccessLog>,
    ) -> Self {
        Self {
            state,
            client,
            ui,
            approval_queue,
            access_log,
            client_pid: 0,
        }
    }

    /// Create a per-session clone with the given client PID.
    pub fn with_client_pid(&self, pid: u32) -> Self {
        let mut handler = self.clone();
        handler.client_pid = pid;
        handler
    }

    /// Get a reference to the approval queue (for Tauri IPC commands).
    pub fn approval_queue(&self) -> &Arc<ApprovalQueue> {
        &self.approval_queue
    }

    /// Get a reference to the access log (for Tauri IPC commands).
    pub fn access_log(&self) -> &Arc<AccessLog> {
        &self.access_log
    }
}

fn agent_error(error: impl std::fmt::Display) -> ssh_agent_lib::error::AgentError {
    ssh_agent_lib::error::AgentError::other(std::io::Error::other(error.to_string()))
}

#[ssh_agent_lib::async_trait]
impl<U: crate::UiCallback> ssh_agent_lib::agent::Session for SshAgentHandler<U> {
    async fn request_identities(
        &mut self,
    ) -> Result<Vec<ssh_agent_lib::proto::Identity>, ssh_agent_lib::error::AgentError> {
        auth::ensure_unlocked(self.state.clone(), &self.client, self.ui.as_ref())
            .await
            .map_err(agent_error)?;

        let state = self.state.lock().await;
        let mut identities = Vec::new();

        for entry in &state.entries {
            if let bw_core::db::EntryData::SshKey {
                public_key: Some(encrypted_pubkey),
                ..
            } = &entry.data
            {
                let plaintext = auth::decrypt_cipher(
                    &state,
                    encrypted_pubkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(agent_error)?;

                let pubkey = ssh_agent_lib::ssh_key::PublicKey::from_openssh(&plaintext)
                    .map_err(agent_error)?;

                identities.push(ssh_agent_lib::proto::Identity {
                    pubkey: pubkey.key_data().clone(),
                    comment: entry.name.clone(),
                });
            }
        }

        Ok(identities)
    }

    async fn sign(
        &mut self,
        request: ssh_agent_lib::proto::SignRequest,
    ) -> Result<ssh_agent_lib::ssh_key::Signature, ssh_agent_lib::error::AgentError> {
        auth::ensure_unlocked(self.state.clone(), &self.client, self.ui.as_ref())
            .await
            .map_err(agent_error)?;

        let requested_pubkey = ssh_agent_lib::ssh_key::PublicKey::from(request.pubkey.clone());
        let requested_bytes = requested_pubkey.to_bytes().map_err(agent_error)?;
        let requested_fingerprint = requested_pubkey.fingerprint(Default::default());

        // Find the matching entry name for the approval request.
        let (key_name, fingerprint_str) = {
            let state = self.state.lock().await;
            let mut found = None;
            for entry in &state.entries {
                if let bw_core::db::EntryData::SshKey {
                    public_key: Some(encrypted_pubkey),
                    ..
                } = &entry.data
                {
                    if let Ok(pubkey_plain) = auth::decrypt_cipher(
                        &state,
                        encrypted_pubkey,
                        entry.key.as_deref(),
                        entry.org_id.as_deref(),
                    ) {
                        if let Ok(pubkey) =
                            ssh_agent_lib::ssh_key::PublicKey::from_openssh(&pubkey_plain)
                        {
                            if let Ok(bytes) = pubkey.to_bytes() {
                                if bytes == requested_bytes {
                                    let decrypted_name = auth::decrypt_cipher(
                                        &state,
                                        &entry.name,
                                        entry.key.as_deref(),
                                        entry.org_id.as_deref(),
                                    )
                                    .unwrap_or_else(|_| entry.name.clone());
                                    found = Some(decrypted_name);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            (
                found.unwrap_or_else(|| "unknown".to_string()),
                requested_fingerprint.to_string(),
            )
        };

        // Resolve full process chain from client PID.
        let process_chain = crate::process::resolve_process_chain(self.client_pid);
        // client_exe = topmost initiator (first in chain).
        let client_exe = process_chain
            .first()
            .map(|p| p.exe.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let (approval_request, approval_rx) = self
            .approval_queue
            .create_request(
                &key_name,
                &fingerprint_str,
                &client_exe,
                self.client_pid,
                process_chain.clone(),
            )
            .await;

        if !self.ui.request_approval(&approval_request).await {
            self.approval_queue
                .respond(&approval_request.id, false)
                .await;
        }

        let approved = approval_rx.await.unwrap_or(false);

        // Log the access regardless of approval result.
        if let Err(e) = self.access_log.record(
            &fingerprint_str,
            &key_name,
            &client_exe,
            self.client_pid,
            approved,
            &process_chain,
        ) {
            log::error!("Failed to write access log: {e}");
        }

        if !approved {
            return Err(ssh_agent_lib::error::AgentError::Other(
                "Sign request denied by user".into(),
            ));
        }

        // Proceed with signing.
        let state = self.state.lock().await;
        for entry in &state.entries {
            if let bw_core::db::EntryData::SshKey {
                private_key: Some(encrypted_privkey),
                public_key: Some(encrypted_pubkey),
                ..
            } = &entry.data
            {
                let pubkey_plain = auth::decrypt_cipher(
                    &state,
                    encrypted_pubkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(agent_error)?;

                let pubkey = ssh_agent_lib::ssh_key::PublicKey::from_openssh(&pubkey_plain)
                    .map_err(agent_error)?;
                let pubkey_bytes = pubkey.to_bytes().map_err(agent_error)?;
                if pubkey_bytes != requested_bytes {
                    continue;
                }

                let privkey_plain = auth::decrypt_cipher(
                    &state,
                    encrypted_privkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(agent_error)?;

                let private_key = ssh_agent_lib::ssh_key::PrivateKey::from_openssh(&privkey_plain)
                    .map_err(agent_error)?;

                return match private_key.key_data() {
                    ssh_agent_lib::ssh_key::private::KeypairData::Ed25519(key) => {
                        key.try_sign(&request.data).map_err(agent_error)
                    }
                    ssh_agent_lib::ssh_key::private::KeypairData::Rsa(key) => {
                        use rsa::sha2::Digest as _;

                        let rsa_key = rsa::RsaPrivateKey::try_from(key).map_err(agent_error)?;
                        let mut rng = ssh_agent_lib::ssh_key::rand_core::OsRng;

                        let (algorithm, sig_bytes) = if request.flags & SSH_AGENT_RSA_SHA2_512 != 0
                        {
                            let digest = rsa::sha2::Sha512::digest(&request.data);
                            let signature = rsa_key
                                .sign_with_rng(
                                    &mut rng,
                                    rsa::Pkcs1v15Sign::new::<rsa::sha2::Sha512>(),
                                    &digest,
                                )
                                .map_err(agent_error)?;
                            ("rsa-sha2-512", signature)
                        } else if request.flags & SSH_AGENT_RSA_SHA2_256 != 0 {
                            let digest = rsa::sha2::Sha256::digest(&request.data);
                            let signature = rsa_key
                                .sign_with_rng(
                                    &mut rng,
                                    rsa::Pkcs1v15Sign::new::<rsa::sha2::Sha256>(),
                                    &digest,
                                )
                                .map_err(agent_error)?;
                            ("rsa-sha2-256", signature)
                        } else {
                            let digest = {
                                use sha1::Digest as _;
                                sha1::Sha1::digest(&request.data)
                            };
                            let signature = rsa_key
                                .sign_with_rng(
                                    &mut rng,
                                    rsa::Pkcs1v15Sign::new_unprefixed(),
                                    &digest,
                                )
                                .map_err(agent_error)?;
                            ("ssh-rsa", signature)
                        };

                        ssh_agent_lib::ssh_key::Signature::new(
                            ssh_agent_lib::ssh_key::Algorithm::new(algorithm)
                                .map_err(agent_error)?,
                            sig_bytes,
                        )
                        .map_err(agent_error)
                    }
                    other => Err(ssh_agent_lib::error::AgentError::Other(
                        format!("Unsupported key type: {other:?}").into(),
                    )),
                };
            }
        }

        Err(ssh_agent_lib::error::AgentError::Other(
            "No matching private key found".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_identities_returns_empty_when_no_entries() {
        let state = Arc::new(Mutex::new(crate::state::State::new(Some(
            std::time::Duration::from_secs(900),
        ))));
        {
            let mut state = state.lock().await;
            let mut keys = bw_core::locked::Vec::new();
            keys.extend(std::iter::repeat_n(0u8, 64));
            state.set_unlocked(
                bw_core::locked::Keys::new(keys),
                std::collections::HashMap::new(),
            );
            state.email = Some("test@example.com".to_string());
        }

        let client = bw_core::api::Client::bitwarden_cloud(None);
        let approval_queue = Arc::new(crate::approval::ApprovalQueue::new());
        let access_log = Arc::new(crate::access_log::AccessLog::open_in_memory().unwrap());
        let mut handler = SshAgentHandler::new(
            state,
            client,
            Arc::new(crate::StubUiCallback),
            approval_queue,
            access_log,
        );

        use ssh_agent_lib::agent::Session;
        let identities = handler
            .request_identities()
            .await
            .expect("empty state should still list identities");

        assert!(identities.is_empty());
    }
}
