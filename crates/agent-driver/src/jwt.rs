//! The App JWT: what GitHub needs to tell us which installation to use and
//! to mint installation tokens. It is a credential for ten minutes.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Seconds to backdate `iat`, for clock drift between here and GitHub.
const BACKDATE: u64 = 60;
/// Lifetime from `iat`. GitHub refuses `exp` more than 10 minutes ahead.
const LIFETIME: u64 = 10 * 60;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub iat: u64,
    pub exp: u64,
    pub iss: String,
}

impl Claims {
    /// Claims for `app_id` at Unix time `now`.
    pub fn at(app_id: u64, now: u64) -> Self {
        let iat = now.saturating_sub(BACKDATE);
        Self {
            iat,
            exp: iat + LIFETIME,
            iss: app_id.to_string(),
        }
    }

    pub fn now(app_id: u64) -> Result<Self, Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Clock)?;
        Ok(Self::at(app_id, now.as_secs()))
    }
}

/// A signed App JWT. Debug redacts it and there is no Display.
pub struct Jwt(String);

impl Jwt {
    pub fn sign(key_pem: &[u8], claims: &Claims) -> Result<Self, Error> {
        let key = EncodingKey::from_rsa_pem(key_pem)?;
        Ok(Self(jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            claims,
            &key,
        )?))
    }

    /// The `Authorization` header value. Only the GitHub client may call it.
    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for Jwt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Jwt(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, Validation};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, LineEnding};

    #[test]
    fn claims_are_backdated_and_short_lived() {
        let claims = Claims::at(123456, 1_700_000_000);
        assert_eq!(claims.iat, 1_700_000_000 - 60);
        assert_eq!(claims.exp - claims.iat, 600);
        assert!(
            claims.exp - 1_700_000_000 < 600,
            "exp must be under 10 min ahead of now"
        );
        assert_eq!(claims.iss, "123456");
    }

    #[test]
    fn claims_near_the_epoch_do_not_underflow() {
        let claims = Claims::at(1, 30);
        assert_eq!(claims.iat, 0);
        assert_eq!(claims.exp, 600);
    }

    #[test]
    fn signs_rs256_that_the_public_key_verifies() {
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap_or_else(|e| panic!("{e}"));
        let private_pem = private
            .to_pkcs1_pem(LineEnding::LF)
            .unwrap_or_else(|e| panic!("{e}"));
        let public_pem = private
            .to_public_key()
            .to_pkcs1_pem(LineEnding::LF)
            .unwrap_or_else(|e| panic!("{e}"));

        let claims = Claims::at(7, 1_700_000_000);
        let jwt = Jwt::sign(private_pem.as_bytes(), &claims).unwrap_or_else(|e| panic!("{e}"));

        let header = jsonwebtoken::decode_header(&jwt.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(header.alg, Algorithm::RS256);

        let key =
            DecodingKey::from_rsa_pem(public_pem.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = false;
        validation.set_required_spec_claims(&["iss"]);
        validation.set_issuer(&["7"]);
        let decoded = jsonwebtoken::decode::<Claims>(&jwt.0, &key, &validation)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(decoded.claims, claims);
    }

    #[test]
    fn debug_redacts() {
        let jwt = Jwt("header.payload.signature".to_owned());
        assert_eq!(format!("{jwt:?}"), "Jwt(<redacted>)");
    }

    #[test]
    fn refuses_a_key_that_is_not_pem() {
        assert!(Jwt::sign(b"not a key", &Claims::at(1, 0)).is_err());
    }
}
