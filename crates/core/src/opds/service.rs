use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use rand::RngExt;

use crate::{Error, repository::RepositoryService, types::Capability, user::User, with_read_only_transaction, with_transaction};

const OPDS_PASSWORD_KEY: &str = "opds_password_hash";
const OPDS_PASSWORD_LENGTH: usize = 12;
const OPDS_PASSWORD_CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";

#[async_trait::async_trait]
pub trait OpdsService: Send + Sync {
    /// Returns the plaintext OPDS password for a user, generating one if none
    /// exists.
    async fn get_or_create_password(&self, user: &User) -> Result<String, Error>;

    /// Generates a new OPDS password, replacing any existing one.
    /// Returns the new plaintext password.
    async fn regenerate_password(&self, user: &User) -> Result<String, Error>;

    /// Verifies a plaintext password against the stored OPDS password hash for
    /// a user.
    async fn verify_password(&self, user: &User, password: &str) -> Result<bool, Error>;

    /// Returns true if the user has a stored OPDS password.
    async fn has_password(&self, user: &User) -> Result<bool, Error>;
}

pub(crate) struct OpdsServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl OpdsServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

fn generate_opds_password() -> String {
    let mut rng = rand::rng();
    (0..OPDS_PASSWORD_LENGTH)
        .map(|_| {
            let idx = rng.random_range(0..OPDS_PASSWORD_CHARSET.len());
            OPDS_PASSWORD_CHARSET[idx] as char
        })
        .collect()
}

fn hash_opds_password(password: &str) -> Result<String, Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::CryptoError(e.to_string()))?;
    Ok(hash.to_string())
}

fn verify_opds_password(password: &str, hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}

#[async_trait::async_trait]
impl OpdsService for OpdsServiceImpl {
    async fn get_or_create_password(&self, user: &User) -> Result<String, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Err(Error::Validation("User does not have OPDS access".to_string()));
        }

        let user_id = user.id;
        let existing = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;

        if existing.is_some() {
            return Err(Error::Validation("OPDS password already exists; use regenerate to get a new one".to_string()));
        }

        let plaintext = generate_opds_password();
        let hash = hash_opds_password(&plaintext)?;

        let setting = crate::user::NewUserSetting {
            user_id,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        };
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.set(tx, setting).await)?;

        Ok(plaintext)
    }

    async fn regenerate_password(&self, user: &User) -> Result<String, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Err(Error::Validation("User does not have OPDS access".to_string()));
        }

        let plaintext = generate_opds_password();
        let hash = hash_opds_password(&plaintext)?;

        let setting = crate::user::NewUserSetting {
            user_id: user.id,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        };
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.set(tx, setting).await)?;

        Ok(plaintext)
    }

    async fn verify_password(&self, user: &User, password: &str) -> Result<bool, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Ok(false);
        }

        let user_id = user.id;
        let setting = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;

        match setting {
            Some(s) => Ok(verify_opds_password(password, &s.value)),
            None => Ok(false),
        }
    }

    async fn has_password(&self, user: &User) -> Result<bool, Error> {
        let user_id = user.id;
        let setting = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;
        Ok(setting.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_opds_password_length() {
        let password = generate_opds_password();
        assert_eq!(password.len(), OPDS_PASSWORD_LENGTH);
    }

    #[test]
    fn test_generate_opds_password_is_alphanumeric() {
        let password = generate_opds_password();
        assert!(password.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_opds_password_uniqueness() {
        let p1 = generate_opds_password();
        let p2 = generate_opds_password();
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_hash_and_verify_round_trip() {
        let password = "testpassword";
        let hash = hash_opds_password(password).unwrap();
        assert!(verify_opds_password(password, &hash));
    }

    #[test]
    fn test_verify_wrong_password() {
        let hash = hash_opds_password("correct").unwrap();
        assert!(!verify_opds_password("wrong", &hash));
    }

    #[test]
    fn test_verify_invalid_hash() {
        assert!(!verify_opds_password("password", "not-a-valid-hash"));
    }
}
