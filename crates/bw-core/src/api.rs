// Minimal stub — full implementation in Task 4-5.
// Only defines TwoFactorProviderType needed by error.rs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
