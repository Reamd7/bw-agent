use crate::prelude::*;

use sha2::Digest as _;

pub struct Identity {
    pub email: String,
    pub keys: crate::locked::Keys,
    pub master_password_hash: crate::locked::PasswordHash,
}

impl Identity {
    pub fn new(
        email: &str,
        password: &crate::locked::Password,
        kdf: crate::api::KdfType,
        iterations: u32,
        memory: Option<u32>,
        parallelism: Option<u32>,
    ) -> Result<Self> {
        let email = email.trim().to_lowercase();

        let iterations =
            std::num::NonZeroU32::new(iterations).ok_or(Error::Pbkdf2ZeroIterations)?;

        let mut keys = crate::locked::Vec::new();
        keys.extend(std::iter::repeat_n(0, 64));

        let enc_key = &mut keys.data_mut()[0..32];

        match kdf {
            crate::api::KdfType::Pbkdf2 => {
                pbkdf2_sha256(
                    password.password(),
                    email.as_bytes(),
                    iterations.get(),
                    enc_key,
                )?;
            }
            crate::api::KdfType::Argon2id => {
                let mut hasher = sha2::Sha256::new();
                hasher.update(email.as_bytes());
                let salt = hasher.finalize();

                let memory = memory.ok_or(Error::Argon2)?;
                let parallelism = parallelism.ok_or(Error::Argon2)?;
                let memory = memory.checked_mul(1024).ok_or(Error::Argon2)?;

                let params = argon2::Params::new(memory, iterations.get(), parallelism, Some(32))
                    .map_err(|_| Error::Argon2)?;
                let argon2 = argon2::Argon2::new(
                    argon2::Algorithm::Argon2id,
                    argon2::Version::V0x13,
                    params,
                );
                argon2
                    .hash_password_into(password.password(), &salt, enc_key)
                    .map_err(|_| Error::Argon2)?;
            }
        }

        let mut hash = crate::locked::Vec::new();
        hash.extend(std::iter::repeat_n(0, 32));
        pbkdf2_sha256(enc_key, password.password(), 1, hash.data_mut())?;

        let hkdf = hkdf::Hkdf::<sha2::Sha256>::from_prk(enc_key).map_err(|_| Error::HkdfExpand)?;
        hkdf.expand(b"enc", enc_key)
            .map_err(|_| Error::HkdfExpand)?;
        let mac_key = &mut keys.data_mut()[32..64];
        hkdf.expand(b"mac", mac_key)
            .map_err(|_| Error::HkdfExpand)?;

        let keys = crate::locked::Keys::new(keys);
        let master_password_hash = crate::locked::PasswordHash::new(hash);

        Ok(Self {
            email,
            keys,
            master_password_hash,
        })
    }
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32, output: &mut [u8]) -> Result<()> {
    use hmac::{KeyInit as _, Mac as _};

    const BLOCK_LEN: usize = 32;

    let mut block_index = 1_u32;
    for chunk in output.chunks_mut(BLOCK_LEN) {
        let mut mac =
            hmac::Hmac::<sha2::Sha256>::new_from_slice(password).map_err(|_| Error::Pbkdf2)?;
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let mut u = mac.finalize().into_bytes();

        let mut t = [0_u8; BLOCK_LEN];
        t.copy_from_slice(&u);

        for _ in 1..rounds {
            let mut mac =
                hmac::Hmac::<sha2::Sha256>::new_from_slice(password).map_err(|_| Error::Pbkdf2)?;
            mac.update(&u);
            u = mac.finalize().into_bytes();

            for (lhs, rhs) in t.iter_mut().zip(u.iter()) {
                *lhs ^= rhs;
            }
        }

        chunk.copy_from_slice(&t[..chunk.len()]);
        block_index = block_index.checked_add(1).ok_or(Error::Pbkdf2)?;
    }

    Ok(())
}
