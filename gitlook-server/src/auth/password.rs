//! 密码哈希：argon2id（Argon2 的推荐变体）。
//!
//! 返回 PHC 字符串（$argon2id$v=19$m=...,t=...,p=...$salt$hash），
//! 自带 salt、memory/cost/time 参数，验证时直接解析即可，无需单独存 salt。

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::rngs::OsRng;

/// 生成密码哈希（argon2id，PHC 格式）。
pub fn hash(plain: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default(); // 使用推荐参数：m=19456, t=2, p=1
    let hash = argon2.hash_password(plain.as_bytes(), &salt).expect("argon2 hash failed");
    hash.to_string()
}

/// 验证明文密码对哈希（PHC 格式）。
pub fn verify(plain: &str, hash_str: &str) -> bool {
    let parsed = match PasswordHash::new(hash_str) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let argon2 = Argon2::default();
    argon2.verify_password(plain.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let plain = "correct-horse-battery-staple";
        let h = hash(plain);
        assert!(verify(plain, &h));
    }

    #[test]
    fn verify_wrong_password_fails() {
        let plain = "correct-horse-battery-staple";
        let h = hash(plain);
        assert!(!verify("wrong-password", &h));
    }

    #[test]
    fn verify_tampered_hash_fails() {
        let h = hash("password123");
        // 改动最后一个字符
        let mut tampered = h.chars().collect::<Vec<_>>();
        if let Some(last) = tampered.last_mut() {
            *last = if *last == 'a' { 'b' } else { 'a' };
        }
        let tampered: String = tampered.into_iter().collect();
        assert!(!verify("password123", &tampered));
    }
}