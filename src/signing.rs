use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::path::Path;

/// Keypair for signing packages
pub struct PackageSigningKey {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl PackageSigningKey {
    /// Generate a new Ed25519 keypair
    pub fn generate() -> Result<Self> {
        let mut csprng = OsRng;

        // Generate 32 random bytes for the secret key
        let mut secret_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut csprng, &mut secret_bytes);

        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Load keypair from PEM files
    pub fn load_from_files(private_path: &Path, public_path: &Path) -> Result<Self> {
        // Read private key
        let private_pem = std::fs::read_to_string(private_path)
            .context("Failed to read private key file")?;

        let private_parsed = pem::parse(&private_pem)
            .context("Failed to parse private key PEM")?;

        if private_parsed.contents().len() != 32 {
            anyhow::bail!("Invalid private key length (expected 32 bytes)");
        }

        let signing_key = SigningKey::from_bytes(
            private_parsed
                .contents()
                .try_into()
                .context("Failed to convert private key")?,
        );

        // Read public key
        let public_pem = std::fs::read_to_string(public_path)
            .context("Failed to read public key file")?;

        let public_parsed = pem::parse(&public_pem)
            .context("Failed to parse public key PEM")?;

        if public_parsed.contents().len() != 32 {
            anyhow::bail!("Invalid public key length (expected 32 bytes)");
        }

        let verifying_key = VerifyingKey::from_bytes(
            public_parsed
                .contents()
                .try_into()
                .context("Failed to convert public key")?,
        )
        .context("Invalid public key")?;

        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Save keypair to PEM files
    pub fn save_to_files(&self, private_path: &Path, public_path: &Path) -> Result<()> {
        // Ensure parent directories exist
        if let Some(parent) = private_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create keys directory")?;
        }

        // Save private key
        let private_pem = pem::Pem::new("PRIVATE KEY", self.signing_key.to_bytes());
        let private_encoded = pem::encode(&private_pem);
        std::fs::write(private_path, private_encoded)
            .context("Failed to write private key")?;

        // Set strict permissions on private key (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(private_path)?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600); // Read/write for owner only
            std::fs::set_permissions(private_path, permissions)?;
        }

        // Save public key
        let public_pem = pem::Pem::new("PUBLIC KEY", self.verifying_key.to_bytes());
        let public_encoded = pem::encode(&public_pem);
        std::fs::write(public_path, public_encoded)
            .context("Failed to write public key")?;

        Ok(())
    }

    /// Sign data and return a 64-byte signature
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    /// Get public key as hex string (for storage in metadata)
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key.to_bytes())
    }

    /// Get public key bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }
}

/// Verify a signature against data using a public key (hex-encoded)
pub fn verify_signature(
    data: &[u8],
    signature_bytes: &[u8],
    public_key_hex: &str,
) -> Result<bool> {
    // Decode public key from hex
    let public_key_bytes = hex::decode(public_key_hex)
        .context("Failed to decode public key from hex")?;

    if public_key_bytes.len() != 32 {
        anyhow::bail!("Invalid public key length (expected 32 bytes, got {})", public_key_bytes.len());
    }

    let verifying_key = VerifyingKey::from_bytes(
        public_key_bytes
            .as_slice()
            .try_into()
            .context("Failed to convert public key")?,
    )
    .context("Invalid public key")?;

    // Parse signature
    if signature_bytes.len() != 64 {
        anyhow::bail!("Invalid signature length (expected 64 bytes, got {})", signature_bytes.len());
    }

    let signature = Signature::from_bytes(
        signature_bytes
            .try_into()
            .context("Failed to convert signature")?,
    );

    // Verify
    match verifying_key.verify(data, &signature) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Load or generate signing keys
///
/// If keys exist, load them. Otherwise, generate new keys and save them.
pub fn load_or_generate_keys(private_path: &Path, public_path: &Path) -> Result<PackageSigningKey> {
    if private_path.exists() && public_path.exists() {
        // Load existing keys
        PackageSigningKey::load_from_files(private_path, public_path)
    } else {
        // Generate new keys
        println!("⚠  No signing keys found. Generating new Ed25519 keypair...");
        let keys = PackageSigningKey::generate()?;
        keys.save_to_files(private_path, public_path)?;

        println!("  ✓ Private key saved to {}", private_path.display());
        println!("  ✓ Public key saved to {}", public_path.display());
        println!();
        println!("⚠  IMPORTANT: Keep your private key safe!");
        println!("  • Never commit it to version control");
        println!("  • Back it up securely");
        println!("  • Don't share it with anyone");
        println!();
        println!("Your public key (share with users):");
        println!("  {}", keys.public_key_hex());
        println!();

        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_generation() {
        let keys = PackageSigningKey::generate().unwrap();
        let public_hex = keys.public_key_hex();

        // Public key should be 64 hex characters (32 bytes)
        assert_eq!(public_hex.len(), 64);
    }

    #[test]
    fn test_sign_and_verify() {
        let keys = PackageSigningKey::generate().unwrap();
        let data = b"Hello, UnrealPM!";

        // Sign
        let signature = keys.sign(data);

        // Verify
        let is_valid = verify_signature(data, &signature.to_bytes(), &keys.public_key_hex()).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let keys = PackageSigningKey::generate().unwrap();
        let data = b"Hello, UnrealPM!";
        let signature = keys.sign(data);

        // Tamper with data
        let tampered_data = b"Hello, UnrealPM!!!";

        // Verify should fail
        let is_valid = verify_signature(tampered_data, &signature.to_bytes(), &keys.public_key_hex()).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_save_and_load_keys() {
        let temp_dir = TempDir::new().unwrap();
        let private_path = temp_dir.path().join("private.pem");
        let public_path = temp_dir.path().join("public.pem");

        // Generate and save
        let original_keys = PackageSigningKey::generate().unwrap();
        original_keys.save_to_files(&private_path, &public_path).unwrap();

        // Load
        let loaded_keys = PackageSigningKey::load_from_files(&private_path, &public_path).unwrap();

        // Verify they produce the same signatures
        let data = b"Test data";
        let original_sig = original_keys.sign(data);
        let loaded_sig = loaded_keys.sign(data);

        assert_eq!(original_sig.to_bytes(), loaded_sig.to_bytes());
    }

    #[test]
    fn test_tampered_file_detected() {
        let keys = PackageSigningKey::generate().unwrap();
        let data = b"Original file content";
        let signature = keys.sign(data);

        // Simulate tampering
        let tampered_data = b"Tampered file content";

        // Verification should fail
        let is_valid = verify_signature(tampered_data, &signature.to_bytes(), &keys.public_key_hex()).unwrap();
        assert!(!is_valid);
    }
}
