use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroizing;

const PASSWORD_MIN_LENGTH: usize = 12;
const PASSWORD_MAX_LENGTH: usize = 128;
const TOKEN_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

type HmacSha256 = Hmac<Sha256>;

pub fn hash_password(password: &SecretString) -> Result<String, AuthError> {
    validate_password(password.expose_secret())?;
    let mut salt_bytes = [0_u8; SALT_BYTES];
    getrandom::fill(&mut salt_bytes).map_err(|_| AuthError::EntropyUnavailable)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AuthError::PasswordHash)?;
    let params = Params::new(65_536, 3, 1, None).map_err(|_| AuthError::PasswordHash)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2
        .hash_password(password.expose_secret().as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHash)
}

pub fn verify_password(password: &SecretString, encoded_hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.expose_secret().as_bytes(), &hash)
        .is_ok()
}

pub fn validate_password(password: &str) -> Result<(), AuthError> {
    let length = password.chars().count();
    if !(PASSWORD_MIN_LENGTH..=PASSWORD_MAX_LENGTH).contains(&length) {
        return Err(AuthError::InvalidPassword);
    }
    if password.chars().any(char::is_control) {
        return Err(AuthError::InvalidPassword);
    }
    Ok(())
}

pub fn generate_token() -> Result<SecretString, AuthError> {
    let mut bytes = Zeroizing::new([0_u8; TOKEN_BYTES]);
    getrandom::fill(bytes.as_mut()).map_err(|_| AuthError::EntropyUnavailable)?;
    Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes.as_ref())))
}

#[must_use]
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub fn csrf_token(session_token: &str, csrf_key: &SecretString) -> Result<SecretString, AuthError> {
    let mut mac = HmacSha256::new_from_slice(csrf_key.expose_secret().as_bytes())
        .map_err(|_| AuthError::InvalidCsrfKey)?;
    mac.update(b"heterocloud-csrf-v1\0");
    mac.update(session_token.as_bytes());
    Ok(SecretString::from(
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()),
    ))
}

#[must_use]
pub fn constant_time_token_eq(left: &str, right: &str) -> bool {
    let left_hash = token_hash(left);
    let right_hash = token_hash(right);
    bool::from(left_hash.ct_eq(&right_hash))
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("cryptographic entropy is unavailable")]
    EntropyUnavailable,
    #[error("password does not satisfy the password policy")]
    InvalidPassword,
    #[error("password hashing failed")]
    PasswordHash,
    #[error("CSRF key must not be empty")]
    InvalidCsrfKey,
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::{
        constant_time_token_eq, csrf_token, generate_token, hash_password, verify_password,
    };

    #[test]
    fn password_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let password = SecretString::from("correct horse battery staple".to_owned());
        let hash = hash_password(&password)?;
        assert!(verify_password(&password, &hash));
        assert!(!verify_password(
            &SecretString::from("incorrect horse battery staple".to_owned()),
            &hash
        ));
        Ok(())
    }

    #[test]
    fn generated_tokens_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let first = generate_token()?;
        let second = generate_token()?;
        assert_ne!(first.expose_secret(), second.expose_secret());
        Ok(())
    }

    #[test]
    fn csrf_is_bound_to_session() -> Result<(), Box<dyn std::error::Error>> {
        let key = SecretString::from("test-key-with-enough-randomness".to_owned());
        let first = csrf_token("session-a", &key)?;
        let second = csrf_token("session-b", &key)?;
        assert!(!constant_time_token_eq(
            first.expose_secret(),
            second.expose_secret()
        ));
        Ok(())
    }
}
