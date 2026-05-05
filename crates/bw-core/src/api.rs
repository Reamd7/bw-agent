use crate::json::{DeserializeJsonWithPath as _, DeserializeJsonWithPathAsync as _};
use crate::prelude::*;

#[allow(clippy::as_conversions)]
#[derive(
    serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(u8)]
pub enum UriMatchType {
    Domain = 0,
    Host = 1,
    StartsWith = 2,
    Exact = 3,
    RegularExpression = 4,
    Never = 5,
}

impl std::fmt::Display for UriMatchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[allow(clippy::enum_glob_use)]
        use UriMatchType::*;
        let s = match self {
            Domain => "domain",
            Host => "host",
            StartsWith => "starts_with",
            Exact => "exact",
            RegularExpression => "regular_expression",
            Never => "never",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TwoFactorProviderType {
    Authenticator = 0,
    Email = 1,
    Duo = 2,
    Yubikey = 3,
    U2f = 4,
    Remember = 5,
    OrganizationDuo = 6,
    WebAuthn = 7,
}

impl TwoFactorProviderType {
    pub fn message(&self) -> &str {
        match *self {
            Self::Authenticator => {
                "Enter the 6 digit verification code from your authenticator app."
            }
            Self::Yubikey => "Insert your Yubikey and push the button.",
            Self::Email => "Enter the PIN you received via email.",
            _ => "Enter the code.",
        }
    }

    pub fn header(&self) -> &str {
        match *self {
            Self::Authenticator => "Authenticator App",
            Self::Yubikey => "Yubikey",
            Self::Email => "Email Code",
            _ => "Two Factor Authentication",
        }
    }

    pub fn grab(&self) -> bool {
        !matches!(self, Self::Email)
    }
}

impl<'de> serde::Deserialize<'de> for TwoFactorProviderType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TwoFactorProviderTypeVisitor;
        impl serde::de::Visitor<'_> for TwoFactorProviderTypeVisitor {
            type Value = TwoFactorProviderType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("two factor provider id")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value.parse().map_err(serde::de::Error::custom)
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                std::convert::TryFrom::try_from(value).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_any(TwoFactorProviderTypeVisitor)
    }
}

impl std::convert::TryFrom<u64> for TwoFactorProviderType {
    type Error = Error;

    fn try_from(ty: u64) -> Result<Self> {
        match ty {
            0 => Ok(Self::Authenticator),
            1 => Ok(Self::Email),
            2 => Ok(Self::Duo),
            3 => Ok(Self::Yubikey),
            4 => Ok(Self::U2f),
            5 => Ok(Self::Remember),
            6 => Ok(Self::OrganizationDuo),
            7 => Ok(Self::WebAuthn),
            _ => Err(Error::InvalidTwoFactorProvider {
                ty: format!("{ty}"),
            }),
        }
    }
}

impl std::str::FromStr for TwoFactorProviderType {
    type Err = Error;

    fn from_str(ty: &str) -> Result<Self> {
        match ty {
            "0" => Ok(Self::Authenticator),
            "1" => Ok(Self::Email),
            "2" => Ok(Self::Duo),
            "3" => Ok(Self::Yubikey),
            "4" => Ok(Self::U2f),
            "5" => Ok(Self::Remember),
            "6" => Ok(Self::OrganizationDuo),
            "7" => Ok(Self::WebAuthn),
            _ => Err(Error::InvalidTwoFactorProvider { ty: ty.to_string() }),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KdfType {
    Pbkdf2 = 0,
    Argon2id = 1,
}

impl<'de> serde::Deserialize<'de> for KdfType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KdfTypeVisitor;
        impl serde::de::Visitor<'_> for KdfTypeVisitor {
            type Value = KdfType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("kdf id")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value.parse().map_err(serde::de::Error::custom)
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                std::convert::TryFrom::try_from(value).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_any(KdfTypeVisitor)
    }
}

impl std::convert::TryFrom<u64> for KdfType {
    type Error = Error;

    fn try_from(ty: u64) -> Result<Self> {
        match ty {
            0 => Ok(Self::Pbkdf2),
            1 => Ok(Self::Argon2id),
            _ => Err(Error::InvalidKdfType {
                ty: format!("{ty}"),
            }),
        }
    }
}

impl std::str::FromStr for KdfType {
    type Err = Error;

    fn from_str(ty: &str) -> Result<Self> {
        match ty {
            "0" => Ok(Self::Pbkdf2),
            "1" => Ok(Self::Argon2id),
            _ => Err(Error::InvalidKdfType { ty: ty.to_string() }),
        }
    }
}

impl serde::Serialize for KdfType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            Self::Pbkdf2 => "0",
            Self::Argon2id => "1",
        };
        serializer.serialize_str(s)
    }
}

#[allow(clippy::as_conversions)]
#[derive(
    serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(u8)]
pub enum CipherRepromptType {
    None = 0,
    Password = 1,
}

#[derive(serde::Serialize, Debug)]
struct PreloginReq {
    email: String,
}

#[derive(serde::Deserialize, Debug)]
struct PreloginRes {
    #[serde(rename = "Kdf", alias = "kdf")]
    kdf: KdfType,
    #[serde(rename = "KdfIterations", alias = "kdfIterations")]
    kdf_iterations: u32,
    #[serde(rename = "KdfMemory", alias = "kdfMemory")]
    kdf_memory: Option<u32>,
    #[serde(rename = "KdfParallelism", alias = "kdfParallelism")]
    kdf_parallelism: Option<u32>,
}

#[derive(serde::Serialize, Debug)]
struct ConnectTokenReq {
    grant_type: String,
    scope: String,
    client_id: String,
    #[serde(rename = "deviceType")]
    device_type: u32,
    #[serde(rename = "deviceIdentifier")]
    device_identifier: String,
    #[serde(rename = "deviceName")]
    device_name: String,
    #[serde(rename = "devicePushToken")]
    device_push_token: String,
    #[serde(rename = "twoFactorToken")]
    two_factor_token: Option<String>,
    #[serde(rename = "twoFactorProvider")]
    two_factor_provider: Option<u32>,
    #[serde(flatten)]
    auth: ConnectTokenAuth,
}

#[derive(serde::Serialize, Debug)]
#[serde(untagged)]
enum ConnectTokenAuth {
    Password(ConnectTokenPassword),
}

#[derive(serde::Serialize, Debug)]
struct ConnectTokenPassword {
    username: String,
    password: String,
}

#[derive(serde::Deserialize, Debug)]
struct ConnectTokenRes {
    access_token: String,
    refresh_token: String,
    #[serde(rename = "Key", alias = "key")]
    key: String,
}

#[derive(serde::Deserialize, Debug)]
struct ConnectErrorRes {
    error: String,
    error_description: Option<String>,
    #[serde(rename = "ErrorModel", alias = "errorModel")]
    error_model: Option<ConnectErrorResErrorModel>,
    #[serde(rename = "TwoFactorProviders", alias = "twoFactorProviders")]
    two_factor_providers: Option<Vec<TwoFactorProviderType>>,
    #[serde(rename = "SsoEmail2faSessionToken", alias = "ssoEmail2faSessionToken")]
    sso_email_2fa_session_token: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct ConnectErrorResErrorModel {
    #[serde(rename = "Message", alias = "message")]
    message: String,
}

#[derive(serde::Serialize, Debug)]
struct ConnectRefreshTokenReq {
    grant_type: String,
    client_id: String,
    refresh_token: String,
}

#[derive(serde::Deserialize, Debug)]
struct ConnectRefreshTokenRes {
    access_token: String,
}

#[derive(serde::Deserialize, Debug)]
struct SyncRes {
    #[serde(rename = "Ciphers", alias = "ciphers")]
    ciphers: Vec<SyncResCipher>,
    #[serde(rename = "Profile", alias = "profile")]
    profile: SyncResProfile,
    #[serde(rename = "Folders", alias = "folders")]
    folders: Vec<SyncResFolder>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct SyncResCipher {
    #[serde(rename = "Id", alias = "id")]
    id: String,
    #[serde(rename = "FolderId", alias = "folderId")]
    folder_id: Option<String>,
    #[serde(rename = "OrganizationId", alias = "organizationId")]
    organization_id: Option<String>,
    #[serde(rename = "Name", alias = "name")]
    name: String,
    #[serde(rename = "Login", alias = "login")]
    login: Option<CipherLogin>,
    #[serde(rename = "Card", alias = "card")]
    card: Option<CipherCard>,
    #[serde(rename = "Identity", alias = "identity")]
    identity: Option<CipherIdentity>,
    #[serde(rename = "SecureNote", alias = "secureNote")]
    secure_note: Option<CipherSecureNote>,
    #[serde(rename = "SshKey", alias = "sshKey")]
    ssh_key: Option<CipherSshKey>,
    #[serde(rename = "Notes", alias = "notes")]
    notes: Option<String>,
    #[serde(rename = "PasswordHistory", alias = "passwordHistory")]
    password_history: Option<Vec<SyncResPasswordHistory>>,
    #[serde(rename = "Fields", alias = "fields")]
    fields: Option<Vec<CipherField>>,
    #[serde(rename = "DeletedDate", alias = "deletedDate")]
    deleted_date: Option<String>,
    #[serde(rename = "Key", alias = "key")]
    key: Option<String>,
    #[serde(rename = "Reprompt", alias = "reprompt")]
    reprompt: CipherRepromptType,
}

impl SyncResCipher {
    fn to_entry(&self, folders: &[SyncResFolder]) -> Option<crate::db::Entry> {
        if self.deleted_date.is_some() {
            return None;
        }
        let history = self
            .password_history
            .as_ref()
            .map_or_else(Vec::new, |history| {
                history
                    .iter()
                    .filter_map(|entry| {
                        entry.password.clone().map(|p| crate::db::HistoryEntry {
                            last_used_date: entry.last_used_date.clone(),
                            password: p,
                        })
                    })
                    .collect()
            });

        let (folder, folder_id) = self.folder_id.as_ref().map_or((None, None), |folder_id| {
            let mut folder_name = None;
            for folder in folders {
                if &folder.id == folder_id {
                    folder_name = Some(folder.name.clone());
                }
            }
            (folder_name, Some(folder_id))
        });
        let data = if let Some(login) = &self.login {
            crate::db::EntryData::Login {
                username: login.username.clone(),
                password: login.password.clone(),
                totp: login.totp.clone(),
                uris: login.uris.as_ref().map_or_else(std::vec::Vec::new, |uris| {
                    uris.iter()
                        .filter_map(|uri| {
                            uri.uri.clone().map(|s| crate::db::Uri {
                                uri: s,
                                match_type: uri.match_type,
                            })
                        })
                        .collect()
                }),
            }
        } else if let Some(card) = &self.card {
            crate::db::EntryData::Card {
                cardholder_name: card.cardholder_name.clone(),
                number: card.number.clone(),
                brand: card.brand.clone(),
                exp_month: card.exp_month.clone(),
                exp_year: card.exp_year.clone(),
                code: card.code.clone(),
            }
        } else if let Some(identity) = &self.identity {
            crate::db::EntryData::Identity {
                title: identity.title.clone(),
                first_name: identity.first_name.clone(),
                middle_name: identity.middle_name.clone(),
                last_name: identity.last_name.clone(),
                address1: identity.address1.clone(),
                address2: identity.address2.clone(),
                address3: identity.address3.clone(),
                city: identity.city.clone(),
                state: identity.state.clone(),
                postal_code: identity.postal_code.clone(),
                country: identity.country.clone(),
                phone: identity.phone.clone(),
                email: identity.email.clone(),
                ssn: identity.ssn.clone(),
                license_number: identity.license_number.clone(),
                passport_number: identity.passport_number.clone(),
                username: identity.username.clone(),
            }
        } else if let Some(_secure_note) = &self.secure_note {
            crate::db::EntryData::SecureNote
        } else if let Some(ssh_key) = &self.ssh_key {
            crate::db::EntryData::SshKey {
                private_key: ssh_key.private_key.clone(),
                public_key: ssh_key.public_key.clone(),
                fingerprint: ssh_key.fingerprint.clone(),
            }
        } else {
            return None;
        };
        let fields = self.fields.as_ref().map_or_else(Vec::new, |fields| {
            fields
                .iter()
                .map(|field| crate::db::Field {
                    ty: field.ty,
                    name: field.name.clone(),
                    value: field.value.clone(),
                    linked_id: field.linked_id,
                })
                .collect()
        });
        Some(crate::db::Entry {
            id: self.id.clone(),
            org_id: self.organization_id.clone(),
            folder,
            folder_id: folder_id.map(std::string::ToString::to_string),
            name: self.name.clone(),
            data,
            fields,
            notes: self.notes.clone(),
            history,
            key: self.key.clone(),
            master_password_reprompt: self.reprompt,
        })
    }
}

#[derive(serde::Deserialize, Debug)]
struct SyncResProfile {
    #[serde(rename = "Key", alias = "key")]
    key: String,
    #[serde(rename = "PrivateKey", alias = "privateKey")]
    private_key: String,
    #[serde(rename = "Organizations", alias = "organizations")]
    organizations: Vec<SyncResProfileOrganization>,
}

#[derive(serde::Deserialize, Debug)]
struct SyncResProfileOrganization {
    #[serde(rename = "Id", alias = "id")]
    id: String,
    #[serde(rename = "Key", alias = "key")]
    key: String,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct SyncResFolder {
    #[serde(rename = "Id", alias = "id")]
    id: String,
    #[serde(rename = "Name", alias = "name")]
    name: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherLogin {
    #[serde(rename = "Username", alias = "username")]
    username: Option<String>,
    #[serde(rename = "Password", alias = "password")]
    password: Option<String>,
    #[serde(rename = "Totp", alias = "totp")]
    totp: Option<String>,
    #[serde(rename = "Uris", alias = "uris")]
    uris: Option<Vec<CipherLoginUri>>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherLoginUri {
    #[serde(rename = "Uri", alias = "uri")]
    uri: Option<String>,
    #[serde(rename = "Match", alias = "match")]
    match_type: Option<UriMatchType>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherCard {
    #[serde(rename = "CardholderName", alias = "cardholderName")]
    cardholder_name: Option<String>,
    #[serde(rename = "Number", alias = "number")]
    number: Option<String>,
    #[serde(rename = "Brand", alias = "brand")]
    brand: Option<String>,
    #[serde(rename = "ExpMonth", alias = "expMonth")]
    exp_month: Option<String>,
    #[serde(rename = "ExpYear", alias = "expYear")]
    exp_year: Option<String>,
    #[serde(rename = "Code", alias = "code")]
    code: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherIdentity {
    #[serde(rename = "Title", alias = "title")]
    title: Option<String>,
    #[serde(rename = "FirstName", alias = "firstName")]
    first_name: Option<String>,
    #[serde(rename = "MiddleName", alias = "middleName")]
    middle_name: Option<String>,
    #[serde(rename = "LastName", alias = "lastName")]
    last_name: Option<String>,
    #[serde(rename = "Address1", alias = "address1")]
    address1: Option<String>,
    #[serde(rename = "Address2", alias = "address2")]
    address2: Option<String>,
    #[serde(rename = "Address3", alias = "address3")]
    address3: Option<String>,
    #[serde(rename = "City", alias = "city")]
    city: Option<String>,
    #[serde(rename = "State", alias = "state")]
    state: Option<String>,
    #[serde(rename = "PostalCode", alias = "postalCode")]
    postal_code: Option<String>,
    #[serde(rename = "Country", alias = "country")]
    country: Option<String>,
    #[serde(rename = "Phone", alias = "phone")]
    phone: Option<String>,
    #[serde(rename = "Email", alias = "email")]
    email: Option<String>,
    #[serde(rename = "SSN", alias = "ssn")]
    ssn: Option<String>,
    #[serde(rename = "LicenseNumber", alias = "licenseNumber")]
    license_number: Option<String>,
    #[serde(rename = "PassportNumber", alias = "passportNumber")]
    passport_number: Option<String>,
    #[serde(rename = "Username", alias = "username")]
    username: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherSshKey {
    #[serde(rename = "PrivateKey", alias = "privateKey")]
    private_key: Option<String>,
    #[serde(rename = "PublicKey", alias = "publicKey")]
    public_key: Option<String>,
    #[serde(rename = "Fingerprint", alias = "keyFingerprint")]
    fingerprint: Option<String>,
}

#[allow(clippy::as_conversions)]
#[derive(
    serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(u16)]
pub enum FieldType {
    Text = 0,
    Hidden = 1,
    Boolean = 2,
    Linked = 3,
}

#[allow(clippy::as_conversions)]
#[derive(
    serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(u16)]
pub enum LinkedIdType {
    LoginUsername = 100,
    LoginPassword = 101,
    CardCardholderName = 300,
    CardExpMonth = 301,
    CardExpYear = 302,
    CardCode = 303,
    CardBrand = 304,
    CardNumber = 305,
    IdentityTitle = 400,
    IdentityMiddleName = 401,
    IdentityAddress1 = 402,
    IdentityAddress2 = 403,
    IdentityAddress3 = 404,
    IdentityCity = 405,
    IdentityState = 406,
    IdentityPostalCode = 407,
    IdentityCountry = 408,
    IdentityCompany = 409,
    IdentityEmail = 410,
    IdentityPhone = 411,
    IdentitySsn = 412,
    IdentityUsername = 413,
    IdentityPassportNumber = 414,
    IdentityLicenseNumber = 415,
    IdentityFirstName = 416,
    IdentityLastName = 417,
    IdentityFullName = 418,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherField {
    #[serde(rename = "Type", alias = "type")]
    ty: Option<FieldType>,
    #[serde(rename = "Name", alias = "name")]
    name: Option<String>,
    #[serde(rename = "Value", alias = "value")]
    value: Option<String>,
    #[serde(rename = "LinkedId", alias = "linkedId")]
    linked_id: Option<LinkedIdType>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CipherSecureNote {}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct SyncResPasswordHistory {
    #[serde(rename = "LastUsedDate", alias = "lastUsedDate")]
    last_used_date: String,
    #[serde(rename = "Password", alias = "password")]
    password: Option<String>,
}

const BITWARDEN_CLIENT: &str = "cli";
const DEVICE_TYPE: u8 = 8;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    identity_url: String,
    proxy: Option<String>,
}

impl Client {
    pub fn new(base_url: &str, identity_url: &str, proxy: Option<&str>) -> Self {
        Self {
            base_url: base_url.to_string(),
            identity_url: identity_url.to_string(),
            proxy: proxy.map(String::from),
        }
    }

    /// Update the base URL, identity URL and proxy at runtime (e.g. after setup).
    pub fn update(&mut self, base_url: &str, identity_url: &str, proxy: Option<&str>) {
        self.base_url = base_url.to_string();
        self.identity_url = identity_url.to_string();
        self.proxy = proxy.map(String::from);
    }

    pub fn bitwarden_cloud(proxy: Option<&str>) -> Self {
        Self::new(
            "https://api.bitwarden.com",
            "https://identity.bitwarden.com",
            proxy,
        )
    }

    fn reqwest_client(&self) -> crate::error::Result<reqwest::Client> {
        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert(
            "Bitwarden-Client-Name",
            reqwest::header::HeaderValue::from_static(BITWARDEN_CLIENT),
        );
        default_headers.insert(
            "Bitwarden-Client-Version",
            reqwest::header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        default_headers.insert(
            "Device-Type",
            reqwest::header::HeaderValue::from_static("8"),
        );

        let mut builder = reqwest::Client::builder()
            .user_agent(format!("bw-agent/{}", env!("CARGO_PKG_VERSION")))
            .default_headers(default_headers);

        if let Some(proxy_url) = &self.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|source| crate::error::Error::CreateReqwestClient { source })?;
            builder = builder.proxy(proxy);
        } else {
            // User didn't configure a proxy — bypass system proxy (e.g. Clash)
            builder = builder.no_proxy();
        }

        builder
            .build()
            .map_err(|source| crate::error::Error::CreateReqwestClient { source })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn identity_url(&self, path: &str) -> String {
        format!("{}{}", self.identity_url, path)
    }

    pub async fn prelogin(
        &self,
        email: &str,
    ) -> crate::error::Result<(KdfType, u32, Option<u32>, Option<u32>)> {
        let prelogin = PreloginReq {
            email: email.to_string(),
        };
        let client = self.reqwest_client()?;
        let res = client
            .post(self.identity_url("/accounts/prelogin"))
            .json(&prelogin)
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;
        let prelogin_res: PreloginRes = res.json_with_path().await?;
        Ok((
            prelogin_res.kdf,
            prelogin_res.kdf_iterations,
            prelogin_res.kdf_memory,
            prelogin_res.kdf_parallelism,
        ))
    }

    pub async fn login(
        &self,
        email: &str,
        device_id: &str,
        password_hash: &crate::locked::PasswordHash,
        two_factor_token: Option<&str>,
        two_factor_provider: Option<TwoFactorProviderType>,
    ) -> crate::error::Result<(String, String, String)> {
        let connect_req = ConnectTokenReq {
            auth: ConnectTokenAuth::Password(ConnectTokenPassword {
                username: email.to_string(),
                password: crate::base64::encode(password_hash.hash()),
            }),
            grant_type: "password".to_string(),
            scope: "api offline_access".to_string(),
            client_id: BITWARDEN_CLIENT.to_string(),
            device_type: u32::from(DEVICE_TYPE),
            device_identifier: device_id.to_string(),
            device_name: "bw-agent".to_string(),
            device_push_token: String::new(),
            two_factor_token: two_factor_token.map(std::string::ToString::to_string),
            two_factor_provider: two_factor_provider.map(|ty| ty as u32),
        };

        let client = self.reqwest_client()?;
        let res = client
            .post(self.identity_url("/connect/token"))
            .form(&connect_req)
            .header("auth-email", crate::base64::encode_url_safe_no_pad(email))
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;

        if res.status() == reqwest::StatusCode::OK {
            let connect_res: ConnectTokenRes = res.json_with_path().await?;
            Ok((
                connect_res.access_token,
                connect_res.refresh_token,
                connect_res.key,
            ))
        } else {
            let code = res.status().as_u16();
            match res.text().await {
                Ok(body) => match body.clone().json_with_path() {
                    Ok(json) => Err(classify_login_error(&json, code)),
                    Err(e) => {
                        log::warn!("{e}: {body}");
                        Err(crate::error::Error::RequestFailed { status: code })
                    }
                },
                Err(e) => {
                    log::warn!("failed to read response body: {e}");
                    Err(crate::error::Error::RequestFailed { status: code })
                }
            }
        }
    }

    pub async fn sync(
        &self,
        access_token: &str,
    ) -> crate::error::Result<(
        String,
        String,
        std::collections::HashMap<String, String>,
        Vec<crate::db::Entry>,
    )> {
        let client = self.reqwest_client()?;
        let res = client
            .get(self.api_url("/sync"))
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Bitwarden-Client-Version", "2024.12.0")
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;
        match res.status() {
            reqwest::StatusCode::OK => {
                let sync_res: SyncRes = res.json_with_path().await?;
                let folders = sync_res.folders.clone();
                let ciphers = sync_res
                    .ciphers
                    .iter()
                    .filter_map(|cipher| cipher.to_entry(&folders))
                    .collect();
                let org_keys = sync_res
                    .profile
                    .organizations
                    .iter()
                    .map(|org| (org.id.clone(), org.key.clone()))
                    .collect();
                Ok((
                    sync_res.profile.key,
                    sync_res.profile.private_key,
                    org_keys,
                    ciphers,
                ))
            }
            reqwest::StatusCode::UNAUTHORIZED => Err(crate::error::Error::RequestUnauthorized),
            _ => Err(crate::error::Error::RequestFailed {
                status: res.status().as_u16(),
            }),
        }
    }

    pub async fn exchange_refresh_token(
        &self,
        refresh_token: &str,
    ) -> crate::error::Result<String> {
        let connect_req = ConnectRefreshTokenReq {
            grant_type: "refresh_token".to_string(),
            client_id: BITWARDEN_CLIENT.to_string(),
            refresh_token: refresh_token.to_string(),
        };
        let client = self.reqwest_client()?;
        let res = client
            .post(self.identity_url("/connect/token"))
            .form(&connect_req)
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;
        let connect_res: ConnectRefreshTokenRes = res.json_with_path().await?;
        Ok(connect_res.access_token)
    }

    pub async fn get_cipher(
        &self,
        access_token: &str,
        cipher_id: &str,
    ) -> crate::error::Result<serde_json::Value> {
        let client = self.reqwest_client()?;
        let res = client
            .get(self.api_url(&format!("/ciphers/{cipher_id}")))
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;

        match res.status() {
            reqwest::StatusCode::OK => res
                .json()
                .await
                .map_err(|source| crate::error::Error::Reqwest { source }),
            reqwest::StatusCode::UNAUTHORIZED => Err(crate::error::Error::RequestUnauthorized),
            _ => Err(crate::error::Error::RequestFailed {
                status: res.status().as_u16(),
            }),
        }
    }

    pub async fn update_cipher(
        &self,
        access_token: &str,
        cipher_id: &str,
        body: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let client = self.reqwest_client()?;
        let res = client
            .put(self.api_url(&format!("/ciphers/{cipher_id}")))
            .header("Authorization", format!("Bearer {access_token}"))
            .json(body)
            .send()
            .await
            .map_err(|source| crate::error::Error::Reqwest { source })?;

        match res.status() {
            reqwest::StatusCode::OK => Ok(()),
            reqwest::StatusCode::UNAUTHORIZED => Err(crate::error::Error::RequestUnauthorized),
            _ => Err(crate::error::Error::RequestFailed {
                status: res.status().as_u16(),
            }),
        }
    }
}

fn classify_login_error(error_res: &ConnectErrorRes, code: u16) -> crate::error::Error {
    if let Some(providers) = &error_res.two_factor_providers {
        if !providers.is_empty() {
            return crate::error::Error::TwoFactorRequired {
                providers: providers.clone(),
                sso_email_2fa_session_token: error_res.sso_email_2fa_session_token.clone(),
            };
        }
    }

    if error_res.error == "invalid_grant" {
        return crate::error::Error::IncorrectPassword {
            message: error_res.error_model.as_ref().map_or_else(
                || error_res.error_description.clone().unwrap_or_default(),
                |m| m.message.clone(),
            ),
        };
    }

    crate::error::Error::RequestFailed { status: code }
}

pub fn generate_device_id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

pub struct LoginSession {
    pub access_token: String,
    pub refresh_token: String,
    pub kdf: KdfType,
    pub iterations: u32,
    pub memory: Option<u32>,
    pub parallelism: Option<u32>,
    pub protected_key: String,
    pub email: String,
    pub identity: crate::identity::Identity,
}

pub async fn full_login(
    client: &Client,
    email: &str,
    password: &crate::locked::Password,
) -> crate::error::Result<LoginSession> {
    let device_id = generate_device_id();
    let (kdf, iterations, memory, parallelism) = client.prelogin(email).await?;

    let identity =
        crate::identity::Identity::new(email, password, kdf, iterations, memory, parallelism)?;

    let (access_token, refresh_token, protected_key) = client
        .login(
            email,
            &device_id,
            &identity.master_password_hash,
            None,
            None,
        )
        .await?;

    Ok(LoginSession {
        access_token,
        refresh_token,
        kdf,
        iterations,
        memory,
        parallelism,
        protected_key,
        email: email.to_string(),
        identity,
    })
}

pub struct SyncData {
    pub protected_key: String,
    pub protected_private_key: String,
    pub org_keys: std::collections::HashMap<String, String>,
    pub entries: Vec<crate::db::Entry>,
}

pub async fn sync_vault(client: &Client, access_token: &str) -> crate::error::Result<SyncData> {
    let (protected_key, protected_private_key, org_keys, entries) =
        client.sync(access_token).await?;
    Ok(SyncData {
        protected_key,
        protected_private_key,
        org_keys,
        entries,
    })
}

pub fn unlock_vault(
    email: &str,
    password: &crate::locked::Password,
    kdf: KdfType,
    iterations: u32,
    memory: Option<u32>,
    parallelism: Option<u32>,
    protected_key: &str,
    protected_private_key: &str,
    protected_org_keys: &std::collections::HashMap<String, String>,
) -> crate::error::Result<(
    crate::locked::Keys,
    std::collections::HashMap<String, crate::locked::Keys>,
)> {
    let identity =
        crate::identity::Identity::new(email, password, kdf, iterations, memory, parallelism)?;

    let protected_key = crate::cipherstring::CipherString::new(protected_key)?;
    let key = match protected_key.decrypt_locked_symmetric(&identity.keys) {
        Ok(master_keys) => crate::locked::Keys::new(master_keys),
        Err(crate::error::Error::InvalidMac) => {
            return Err(crate::error::Error::IncorrectPassword {
                message: "Password is incorrect. Try again.".to_string(),
            });
        }
        Err(e) => return Err(e),
    };

    let protected_private_key = crate::cipherstring::CipherString::new(protected_private_key)?;
    let private_key =
        crate::locked::PrivateKey::new(protected_private_key.decrypt_locked_symmetric(&key)?);

    let mut org_keys_map = std::collections::HashMap::new();
    for (org_id, protected_org_key) in protected_org_keys {
        let protected_org_key = crate::cipherstring::CipherString::new(protected_org_key)?;
        let org_key =
            crate::locked::Keys::new(protected_org_key.decrypt_locked_asymmetric(&private_key)?);
        org_keys_map.insert(org_id.clone(), org_key);
    }

    Ok((key, org_keys_map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_bitwarden_cloud_urls() {
        let client = Client::bitwarden_cloud(None);
        assert_eq!(client.api_url("/sync"), "https://api.bitwarden.com/sync");
        assert_eq!(
            client.identity_url("/connect/token"),
            "https://identity.bitwarden.com/connect/token"
        );
    }

    #[test]
    fn test_client_with_proxy_builds() {
        let client = Client::new(
            "https://api.bitwarden.com",
            "https://identity.bitwarden.com",
            Some("http://127.0.0.1:7890"),
        );
        let _ = client.reqwest_client().unwrap();
    }

    #[test]
    fn test_classify_login_error_incorrect_password() {
        let error_res = ConnectErrorRes {
            error: "invalid_grant".to_string(),
            error_description: Some("invalid_username_or_password".to_string()),
            error_model: Some(ConnectErrorResErrorModel {
                message: "Username or password is incorrect.".to_string(),
            }),
            two_factor_providers: None,
            sso_email_2fa_session_token: None,
        };
        let err = classify_login_error(&error_res, 400);
        assert!(matches!(err, crate::error::Error::IncorrectPassword { .. }));
    }

    #[test]
    fn test_device_id_is_uuid_format() {
        let id = generate_device_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }
}
