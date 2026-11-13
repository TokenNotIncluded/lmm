//! Lowercase SHA-256 representation shared by fingerprints and lifecycle receipts.
use std::io;

pub(crate) fn digest(value: impl AsRef<[u8]>) -> io::Result<String> {
    let mut output = [0_u8; 64];
    zhex::encode_lower(value.as_ref(), &mut output)
        .map_err(|_| io::Error::other("SHA-256 hex output capacity mismatch"))?;
    String::from_utf8(output.to_vec()).map_err(io::Error::other)
}
