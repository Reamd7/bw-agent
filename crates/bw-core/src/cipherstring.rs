use crate::prelude::*;

use aes::cipher::{BlockModeDecrypt as _, BlockModeEncrypt as _, KeyIvInit as _};
use hmac::{KeyInit as _, Mac as _};
use pkcs8::DecodePrivateKey as _;
use rand::Rng as _;
use rsa::traits::{PrivateKeyParts as _, PublicKeyParts as _};
use sha1::Digest as _;
use zeroize::Zeroize as _;

pub enum CipherString {
    Symmetric {
        // ty: 2 (AES_256_CBC_HMAC_SHA256)
        iv: Vec<u8>,
        ciphertext: Vec<u8>,
        mac: Option<Vec<u8>>,
    },
    Asymmetric {
        // ty: 4 (RSA_2048_OAEP_SHA1)
        ciphertext: Vec<u8>,
    },
}

impl CipherString {
    pub fn new(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 2 {
            return Err(Error::InvalidCipherString {
                reason: "couldn't find type".to_string(),
            });
        }

        let ty = parts[0].as_bytes();
        if ty.len() != 1 {
            return Err(Error::UnimplementedCipherStringType {
                ty: parts[0].to_string(),
            });
        }

        let ty = ty[0] - b'0';
        let contents = parts[1];

        match ty {
            2 => {
                let parts: Vec<&str> = contents.split('|').collect();
                if parts.len() < 2 || parts.len() > 3 {
                    return Err(Error::InvalidCipherString {
                        reason: format!("type 2 cipherstring with {} parts", parts.len()),
                    });
                }

                let iv = crate::base64::decode(parts[0])
                    .map_err(|source| Error::InvalidBase64 { source })?;
                let ciphertext = crate::base64::decode(parts[1])
                    .map_err(|source| Error::InvalidBase64 { source })?;
                let mac = if parts.len() > 2 {
                    Some(
                        crate::base64::decode(parts[2])
                            .map_err(|source| Error::InvalidBase64 { source })?,
                    )
                } else {
                    None
                };

                Ok(Self::Symmetric {
                    iv,
                    ciphertext,
                    mac,
                })
            }
            4 | 6 => {
                let contents = contents.split('|').next().unwrap_or_default();
                let ciphertext = crate::base64::decode(contents)
                    .map_err(|source| Error::InvalidBase64 { source })?;
                Ok(Self::Asymmetric { ciphertext })
            }
            _ => {
                if ty < 6 {
                    Err(Error::TooOldCipherStringType { ty: ty.to_string() })
                } else {
                    Err(Error::UnimplementedCipherStringType { ty: ty.to_string() })
                }
            }
        }
    }

    pub fn encrypt_symmetric(keys: &crate::locked::Keys, plaintext: &[u8]) -> Result<Self> {
        let iv = random_iv();

        let cipher = cbc::Encryptor::<aes::Aes256>::new_from_slices(keys.enc_key(), &iv)
            .map_err(|source| Error::CreateBlockMode { source })?;
        let ciphertext = cipher.encrypt_padded_vec::<block_padding::Pkcs7>(plaintext);

        let mut digest = hmac::Hmac::<sha2::Sha256>::new_from_slice(keys.mac_key())
            .map_err(|source| Error::CreateHmac { source })?;
        digest.update(&iv);
        digest.update(&ciphertext);
        let mac = digest.finalize().into_bytes().as_slice().to_vec();

        Ok(Self::Symmetric {
            iv,
            ciphertext,
            mac: Some(mac),
        })
    }

    pub fn decrypt_symmetric(
        &self,
        keys: &crate::locked::Keys,
        entry_key: Option<&crate::locked::Keys>,
    ) -> Result<Vec<u8>> {
        if let Self::Symmetric {
            iv,
            ciphertext,
            mac,
        } = self
        {
            let cipher = decrypt_common_symmetric(
                entry_key.unwrap_or(keys),
                iv,
                ciphertext,
                mac.as_deref(),
            )?;
            cipher
                .decrypt_padded_vec::<block_padding::Pkcs7>(ciphertext)
                .map_err(|source| Error::Decrypt { source })
        } else {
            Err(Error::InvalidCipherString {
                reason: "found an asymmetric cipherstring, expecting symmetric".to_string(),
            })
        }
    }

    pub fn decrypt_locked_symmetric(
        &self,
        keys: &crate::locked::Keys,
    ) -> Result<crate::locked::Vec> {
        if let Self::Symmetric {
            iv,
            ciphertext,
            mac,
        } = self
        {
            let mut res = crate::locked::Vec::new();
            res.extend(ciphertext.iter().copied());
            let cipher = decrypt_common_symmetric(keys, iv, ciphertext, mac.as_deref())?;
            let plaintext_len = {
                let plaintext = cipher
                    .decrypt_padded::<block_padding::Pkcs7>(res.data_mut())
                    .map_err(|source| Error::Decrypt { source })?;
                plaintext.len()
            };
            res.truncate(plaintext_len);
            Ok(res)
        } else {
            Err(Error::InvalidCipherString {
                reason: "found an asymmetric cipherstring, expecting symmetric".to_string(),
            })
        }
    }

    pub fn decrypt_locked_asymmetric(
        &self,
        private_key: &crate::locked::PrivateKey,
    ) -> Result<crate::locked::Vec> {
        if let Self::Asymmetric { ciphertext } = self {
            let privkey_data = private_key.private_key();
            let privkey_data = pkcs7_unpad(privkey_data).ok_or(Error::Padding)?;
            let pkey = rsa::RsaPrivateKey::from_pkcs8_der(privkey_data)
                .map_err(|source| Error::RsaPkcs8 { source })?;
            let mut bytes =
                rsa_oaep_sha1_decrypt(&pkey, ciphertext).map_err(|source| Error::Rsa { source })?;

            let mut res = crate::locked::Vec::new();
            res.extend(bytes.iter().copied());
            bytes.zeroize();

            Ok(res)
        } else {
            Err(Error::InvalidCipherString {
                reason: "found a symmetric cipherstring, expecting asymmetric".to_string(),
            })
        }
    }
}

fn decrypt_common_symmetric(
    keys: &crate::locked::Keys,
    iv: &[u8],
    ciphertext: &[u8],
    mac: Option<&[u8]>,
) -> Result<cbc::Decryptor<aes::Aes256>> {
    if let Some(mac) = mac {
        let mut key = hmac::Hmac::<sha2::Sha256>::new_from_slice(keys.mac_key())
            .map_err(|source| Error::CreateHmac { source })?;
        key.update(iv);
        key.update(ciphertext);

        key.verify_slice(mac).map_err(|_| Error::InvalidMac)?;
    }

    cbc::Decryptor::<aes::Aes256>::new_from_slices(keys.enc_key(), iv)
        .map_err(|source| Error::CreateBlockMode { source })
}

impl std::fmt::Display for CipherString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Symmetric {
                iv,
                ciphertext,
                mac,
            } => {
                let iv = crate::base64::encode(iv);
                let ciphertext = crate::base64::encode(ciphertext);
                if let Some(mac) = mac {
                    let mac = crate::base64::encode(mac);
                    write!(f, "2.{iv}|{ciphertext}|{mac}")
                } else {
                    write!(f, "2.{iv}|{ciphertext}")
                }
            }
            Self::Asymmetric { ciphertext } => {
                let ciphertext = crate::base64::encode(ciphertext);
                write!(f, "4.{ciphertext}")
            }
        }
    }
}

fn random_iv() -> Vec<u8> {
    let mut iv = vec![0_u8; 16];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut iv);
    iv
}

fn pkcs7_unpad(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.is_empty() {
        return None;
    }

    let padding_val = bytes[bytes.len() - 1];
    if padding_val == 0 {
        return None;
    }

    let padding_len = usize::from(padding_val);
    if padding_len > bytes.len() {
        return None;
    }

    for c in bytes.iter().copied().skip(bytes.len() - padding_len) {
        if c != padding_val {
            return None;
        }
    }

    Some(&bytes[..bytes.len() - padding_len])
}

fn rsa_oaep_sha1_decrypt(
    private_key: &rsa::RsaPrivateKey,
    ciphertext: &[u8],
) -> std::result::Result<Vec<u8>, rsa::errors::Error> {
    let c = rsa::BigUint::from_bytes_be(ciphertext);
    if &c >= private_key.n() {
        return Err(rsa::errors::Error::Decryption);
    }

    let mut encoded_message = c.modpow(private_key.d(), private_key.n()).to_bytes_be();
    let modulus_size = private_key.size();
    if encoded_message.len() > modulus_size {
        return Err(rsa::errors::Error::Decryption);
    }
    if encoded_message.len() < modulus_size {
        let mut padded = vec![0_u8; modulus_size - encoded_message.len()];
        padded.extend_from_slice(&encoded_message);
        encoded_message = padded;
    }

    oaep_sha1_unpad(&encoded_message)
}

fn oaep_sha1_unpad(encoded_message: &[u8]) -> std::result::Result<Vec<u8>, rsa::errors::Error> {
    const HASH_LEN: usize = 20;

    if encoded_message.len() < (HASH_LEN * 2) + 2 {
        return Err(rsa::errors::Error::Decryption);
    }
    if encoded_message[0] != 0 {
        return Err(rsa::errors::Error::Decryption);
    }

    let mut seed = encoded_message[1..1 + HASH_LEN].to_vec();
    let mut data_block = encoded_message[1 + HASH_LEN..].to_vec();

    mgf1_xor(&mut seed, &data_block);
    mgf1_xor(&mut data_block, &seed);

    let label_hash = sha1::Sha1::digest([]);
    if data_block[..HASH_LEN] != label_hash[..] {
        return Err(rsa::errors::Error::Decryption);
    }

    let message_start = data_block[HASH_LEN..]
        .iter()
        .position(|&byte| byte == 1)
        .and_then(|index| {
            data_block[HASH_LEN..HASH_LEN + index]
                .iter()
                .all(|&byte| byte == 0)
                .then_some(HASH_LEN + index + 1)
        })
        .ok_or(rsa::errors::Error::Decryption)?;

    Ok(data_block[message_start..].to_vec())
}

fn mgf1_xor(target: &mut [u8], seed: &[u8]) {
    let mut counter = 0_u32;
    let mut offset = 0_usize;

    while offset < target.len() {
        let mut hasher = sha1::Sha1::new();
        hasher.update(seed);
        hasher.update(counter.to_be_bytes());
        let digest = hasher.finalize();

        for (target_byte, digest_byte) in target[offset..].iter_mut().zip(digest.iter()) {
            *target_byte ^= digest_byte;
        }

        offset += digest.len();
        counter = counter.wrapping_add(1);
    }
}

#[test]
fn test_pkcs7_unpad() {
    let tests = [
        (&[][..], None),
        (&[0x01][..], Some(&[][..])),
        (&[0x02, 0x02][..], Some(&[][..])),
        (&[0x03, 0x03, 0x03][..], Some(&[][..])),
        (&[0x69, 0x01][..], Some(&[0x69][..])),
        (&[0x69, 0x02, 0x02][..], Some(&[0x69][..])),
        (&[0x69, 0x03, 0x03, 0x03][..], Some(&[0x69][..])),
        (&[0x02][..], None),
        (&[0x03][..], None),
        (&[0x69, 0x69, 0x03, 0x03][..], None),
        (&[0x00][..], None),
        (&[0x02, 0x00][..], None),
    ];
    for (input, expected) in tests {
        let got = pkcs7_unpad(input);
        assert_eq!(got, expected);
    }
}
