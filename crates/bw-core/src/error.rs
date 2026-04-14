#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create block mode decryptor")]
    CreateBlockMode { source: aes::cipher::InvalidLength },

    #[error("failed to create HMAC")]
    CreateHmac { source: aes::cipher::InvalidLength },

    #[error("failed to create reqwest client")]
    CreateReqwestClient { source: reqwest::Error },

    #[error("failed to decrypt")]
    Decrypt { source: block_padding::Error },

    #[error("failed to expand with hkdf")]
    HkdfExpand,

    #[error("{message}")]
    IncorrectPassword { message: String },

    #[error("invalid base64")]
    InvalidBase64 { source: base64::DecodeError },

    #[error("invalid cipherstring: {reason}")]
    InvalidCipherString { reason: String },

    #[error("invalid mac")]
    InvalidMac,

    #[error("invalid kdf type: {ty}")]
    InvalidKdfType { ty: String },

    #[error("failed to parse JSON")]
    Json {
        source: serde_path_to_error::Error<serde_json::Error>,
    },

    #[error("invalid padding")]
    Padding,

    #[error("pbkdf2 requires at least 1 iteration (got 0)")]
    Pbkdf2ZeroIterations,

    #[error("failed to run pbkdf2")]
    Pbkdf2,

    #[error("failed to run argon2")]
    Argon2,

    #[error("api request returned error: {status}")]
    RequestFailed { status: u16 },

    #[error("api request unauthorized")]
    RequestUnauthorized,

    #[error("error making api request")]
    Reqwest { source: reqwest::Error },

    #[error("failed to decrypt RSA")]
    Rsa { source: rsa::errors::Error },

    #[error("failed to parse RSA PKCS8")]
    RsaPkcs8 { source: rsa::pkcs8::Error },

    #[error("cipherstring type {ty} too old\n\nPlease rotate your account encryption key (https://bitwarden.com/help/article/account-encryption-key/) and try again.")]
    TooOldCipherStringType { ty: String },

    #[error("invalid two factor provider type: {ty}")]
    InvalidTwoFactorProvider { ty: String },

    #[error("two factor required")]
    TwoFactorRequired {
        providers: Vec<crate::api::TwoFactorProviderType>,
        sso_email_2fa_session_token: Option<String>,
    },

    #[error("unimplemented cipherstring type: {ty}")]
    UnimplementedCipherStringType { ty: String },
}

pub type Result<T> = std::result::Result<T, Error>;
