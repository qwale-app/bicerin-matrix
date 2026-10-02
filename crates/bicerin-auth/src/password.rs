pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    use argon2::{
        password_hash::{PasswordHasher, SaltString},
        Argon2,
    };
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::{hash_password, verify_password};

    #[test]
    fn password_hash_verifies_only_the_original_password() {
        let hash = hash_password("correct horse battery staple").expect("hash password");

        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("incorrect password", &hash));
    }

    #[test]
    fn malformed_password_hash_is_rejected() {
        assert!(!verify_password("password", "not-an-argon2-hash"));
    }
}
