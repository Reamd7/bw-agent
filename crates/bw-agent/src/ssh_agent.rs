use crate::auth;
use crate::state::State;
use signature::Signer as _;
use std::sync::Arc;
use tokio::sync::Mutex;

const SSH_AGENT_RSA_SHA2_256: u32 = 2;
const SSH_AGENT_RSA_SHA2_512: u32 = 4;

#[derive(Clone)]
pub struct SshAgentHandler {
    state: Arc<Mutex<State>>,
    client: bw_core::api::Client,
}

impl SshAgentHandler {
    pub fn new(state: Arc<Mutex<State>>, client: bw_core::api::Client) -> Self {
        Self { state, client }
    }
}

fn agent_error(error: impl std::fmt::Display) -> ssh_agent_lib::error::AgentError {
    ssh_agent_lib::error::AgentError::other(std::io::Error::other(error.to_string()))
}

#[ssh_agent_lib::async_trait]
impl ssh_agent_lib::agent::Session for SshAgentHandler {
    async fn request_identities(
        &mut self,
    ) -> Result<Vec<ssh_agent_lib::proto::Identity>, ssh_agent_lib::error::AgentError> {
        auth::ensure_unlocked(self.state.clone(), &self.client)
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
        auth::ensure_unlocked(self.state.clone(), &self.client)
            .await
            .map_err(agent_error)?;

        let requested_pubkey = ssh_agent_lib::ssh_key::PublicKey::from(request.pubkey.clone());
        let requested_bytes = requested_pubkey.to_bytes().map_err(agent_error)?;

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
                    ssh_agent_lib::ssh_key::private::KeypairData::Ed25519(key) => key
                        .try_sign(&request.data)
                        .map_err(agent_error),
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
                                .sign_with_rng(&mut rng, rsa::Pkcs1v15Sign::new_unprefixed(), &digest)
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
        let state = Arc::new(Mutex::new(crate::state::State::new(
            std::time::Duration::from_secs(900),
        )));
        {
            let mut state = state.lock().await;
            let mut keys = bw_core::locked::Vec::new();
            keys.extend(std::iter::repeat_n(0u8, 64));
            state.set_unlocked(bw_core::locked::Keys::new(keys), std::collections::HashMap::new());
            state.email = Some("test@example.com".to_string());
        }

        let client = bw_core::api::Client::bitwarden_cloud(None);
        let mut handler = SshAgentHandler::new(state, client);

        use ssh_agent_lib::agent::Session;
        let identities = handler
            .request_identities()
            .await
            .expect("empty state should still list identities");

        assert!(identities.is_empty());
    }
}
