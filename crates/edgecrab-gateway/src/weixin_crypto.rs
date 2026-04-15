//! # Weixin & WeCom AES crypto utilities
//!
//! Shared encryption/decryption for both adapters:
//! - **Weixin CDN**: AES-128-ECB with PKCS7 padding
//! - **WeCom media**: AES-256-CBC with PKCS7 padding

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};

type Aes128EcbEnc = ecb::Encryptor<aes::Aes128>;
type Aes128EcbDec = ecb::Decryptor<aes::Aes128>;
#[allow(dead_code)]
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// AES-128-ECB encrypt with PKCS7 padding (Weixin CDN upload).
pub fn aes128_ecb_encrypt(key: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
    Aes128EcbEnc::new(key.into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext)
}

/// AES-128-ECB decrypt with PKCS7 unpadding (Weixin CDN download).
pub fn aes128_ecb_decrypt(key: &[u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>, &'static str> {
    Aes128EcbDec::new(key.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| "AES-128-ECB decryption failed")
}

/// AES-256-CBC decrypt with PKCS7 unpadding (WeCom media download).
///
/// IV is derived from key[0..16] per the WeCom protocol.
pub fn aes256_cbc_decrypt(key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>, &'static str> {
    let iv: &[u8; 16] = key[..16]
        .try_into()
        .map_err(|_| "IV extraction failed")?;
    Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| "AES-256-CBC decryption failed")
}

/// Parse an AES key that may be hex-encoded (32 chars → 16 bytes)
/// or raw bytes (16 bytes).
pub fn parse_aes128_key(input: &[u8]) -> Result<[u8; 16], &'static str> {
    if input.len() == 16 {
        let mut key = [0u8; 16];
        key.copy_from_slice(input);
        Ok(key)
    } else if input.len() == 32 {
        // Hex-encoded
        let hex_str = std::str::from_utf8(input).map_err(|_| "invalid hex key")?;
        let bytes = hex_decode(hex_str)?;
        if bytes.len() != 16 {
            return Err("hex key does not decode to 16 bytes");
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(&bytes);
        Ok(key)
    } else {
        Err("AES key must be 16 bytes or 32 hex chars")
    }
}

fn hex_decode(s: &str) -> Result<Vec<u8>, &'static str> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex string");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "invalid hex char"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes128_ecb_roundtrip() {
        let key = [0x42u8; 16];
        let plaintext = b"Hello, WeChat CDN!";
        let ct = aes128_ecb_encrypt(&key, plaintext);
        let pt = aes128_ecb_decrypt(&key, &ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes128_ecb_roundtrip_empty() {
        let key = [0xAAu8; 16];
        let ct = aes128_ecb_encrypt(&key, b"");
        let pt = aes128_ecb_decrypt(&key, &ct).unwrap();
        assert_eq!(pt, b"");
    }

    #[test]
    fn aes128_ecb_wrong_key() {
        let key = [0x42u8; 16];
        let ct = aes128_ecb_encrypt(&key, b"secret data");
        let wrong_key = [0x99u8; 16];
        // Wrong key should fail or produce garbage
        let result = aes128_ecb_decrypt(&wrong_key, &ct);
        // Either fails or produces different result
        match result {
            Err(_) => {} // expected
            Ok(pt) => assert_ne!(pt, b"secret data"),
        }
    }

    #[test]
    fn aes256_cbc_decrypt_basic() {
        let key = [0x55u8; 32];
        let iv: [u8; 16] = key[..16].try_into().unwrap();
        let plaintext = b"WeCom media data";

        // Encrypt with CBC
        let ct_vec = Aes256CbcEnc::new(&key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(plaintext);

        // Decrypt
        let pt = aes256_cbc_decrypt(&key, &ct_vec).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn parse_key_raw_16() {
        let raw = [0x42u8; 16];
        let key = parse_aes128_key(&raw).unwrap();
        assert_eq!(key, raw);
    }

    #[test]
    fn parse_key_hex_32() {
        let hex = b"4242424242424242424242424242424242424242424242424242424242424242";
        // This is 64 hex chars → 32 bytes, should fail for 16-byte key
        assert!(parse_aes128_key(hex).is_err());

        // 32 hex chars → 16 bytes
        let hex16 = b"42424242424242424242424242424242";
        let key = parse_aes128_key(hex16).unwrap();
        assert_eq!(key, [0x42u8; 16]);
    }

    #[test]
    fn parse_key_invalid_length() {
        assert!(parse_aes128_key(b"short").is_err());
    }
}
