use crate::*;
pub const PASSWORD_REQUIRED: &str = "This PDF requires a password, or the password is incorrect";
impl Engine {
    pub fn open_with_password(&self, path: &Path, password: &str) -> Result<Document> {
        guarded(|| {
            if password.len() > 1024 || password.contains('\0') {
                return Err(err("Invalid password length"));
            }
            let original = path.canonicalize()?;
            let mut bytes = read_limited(&original, MAX_FILE)?;
            let mut options = lopdf::LoadOptions::with_max_decompressed_size(MAX_STREAM);
            options.password = Some(password.to_owned());
            let mut pdf =
                lopdf::Document::load_mem_with_options(&bytes, options).map_err(|e| match e {
                    lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_) => {
                        err(PASSWORD_REQUIRED)
                    }
                    other => other.into(),
                })?;
            let protected = pdf.was_encrypted() || pdf.is_encrypted();
            if protected {
                if pdf.is_encrypted() {
                    pdf.decrypt(password).map_err(|_| err(PASSWORD_REQUIRED))?;
                }
                pdf.prune_objects();
                bytes.clear();
                pdf.save_to(&mut bytes)?;
            }
            parse(&bytes)?;
            let document = Document {
                bytes,
                original,
                undo: vec![],
                redo: vec![],
                revision: 0,
                protected,
                substitution: Substitution::Line,
            };
            self.pages(&document)?;
            Ok(document)
        })
    }
}
impl Document {
    /// Password-protected sources never produce plaintext recovery snapshots.
    pub fn is_password_protected(&self) -> bool {
        self.protected
    }
    pub fn save_as_with_password(&self, path: &Path, password: &str) -> Result<()> {
        guarded(|| {
            if password.is_empty() {
                return self.save_as(path);
            }
            if password.len() > 127 || password.contains('\0') {
                return Err(err("Save password must contain at most 127 UTF-8 bytes"));
            }
            let mut pdf = parse(&self.bytes)?;
            let key = random_key()?;
            let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V5 {
                encrypt_metadata: true,
                crypt_filters: std::collections::BTreeMap::from([(
                    b"StdCF".to_vec(),
                    std::sync::Arc::new(lopdf::encryption::crypt_filters::Aes256CryptFilter)
                        as std::sync::Arc<dyn lopdf::encryption::crypt_filters::CryptFilter>,
                )]),
                file_encryption_key: &key,
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: password,
                user_password: password,
                permissions: lopdf::Permissions::all(),
            })?;
            pdf.encrypt(&encryption)?;
            let mut bytes = vec![];
            pdf.save_to(&mut bytes)?;
            if bytes.len() > MAX_FILE {
                return Err(err("Encrypted PDF exceeds 64 MiB"));
            }
            let export = Document {
                bytes,
                original: self.original.clone(),
                undo: vec![],
                redo: vec![],
                revision: self.revision,
                protected: true,
                substitution: self.substitution,
            };
            export.save_as(path)
        })
    }
}
#[cfg(windows)]
fn random_key() -> Result<[u8; 32]> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            provider: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    let mut key = [0; 32];
    if unsafe { BCryptGenRandom(std::ptr::null_mut(), key.as_mut_ptr(), 32, 2) } < 0 {
        return Err(err("Windows could not generate an encryption key"));
    }
    Ok(key)
}
#[cfg(not(windows))]
fn random_key() -> Result<[u8; 32]> {
    Err(err("Encrypted Save As requires Windows"))
}
