//! Bounded file probes. Recognizing a header does not import board geometry.

use std::{fs::File, io::Read, path::Path};

use crate::{
    ImportContext, ImportError, ImportOptions,
    allegro::header::{BrdHeader, HEADER_BYTES},
};

/// Hash the same owned source bytes that the database parses, without rereading
/// a path that could have changed. Cancellation is checked every 1 MiB.
pub fn content_sha256(bytes: &[u8], context: &ImportContext<'_>) -> Result<[u8; 32], ImportError> {
    use sha2::{Digest, Sha256};
    context.check_cancelled()?;
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(1024 * 1024) {
        context.check_cancelled()?;
        hash.update(chunk);
    }
    context.check_cancelled()?;
    Ok(hash.finalize().into())
}

/// Own one byte buffer. Metadata and streaming growth both obey the input budget.
/// Cancellation is checked between 1 MiB reads and after the final progress callback.
pub fn read_path(
    path: &Path,
    options: &ImportOptions,
    context: &ImportContext<'_>,
) -> Result<Vec<u8>, ImportError> {
    use pomelo_core::task::{ImportProgress, ImportStage};
    use std::time::{Duration, Instant};

    context.check_cancelled()?;
    let mut file = File::open(path)?;
    let expected = file.metadata()?.len();
    let limit = options.max_file_bytes.min(u32::MAX as u64);
    if expected > limit {
        return Err(ImportError::ResourceLimit {
            actual: expected,
            limit,
        });
    }
    // Do not reserve the full, untrusted metadata length before actually reading it.
    let mut bytes = Vec::new();
    let mut chunk = vec![0; 1024 * 1024];
    let mut last = Instant::now();
    (context.progress)(ImportProgress {
        stage: ImportStage::Reading,
        completed: 0,
        total: Some(expected),
    });
    loop {
        context.check_cancelled()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let actual = bytes.len() as u64 + count as u64;
        if actual > limit {
            return Err(ImportError::ResourceLimit { actual, limit });
        }
        if actual > bytes.capacity() as u64 {
            // Grow only after a successful read. Metadata caps spare capacity
            // near EOF, but never truncates a file that grew after the stat.
            let growth_limit = if actual <= expected { expected } else { limit };
            let target = actual.max(
                (bytes.capacity() as u64)
                    .saturating_mul(2)
                    .min(growth_limit)
                    .min(limit),
            );
            bytes.reserve_exact(target as usize - bytes.len());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if last.elapsed() >= Duration::from_millis(100) {
            (context.progress)(ImportProgress {
                stage: ImportStage::Reading,
                completed: bytes.len() as u64,
                total: Some(expected.max(bytes.len() as u64)),
            });
            last = Instant::now();
        }
    }
    (context.progress)(ImportProgress {
        stage: ImportStage::Reading,
        completed: bytes.len() as u64,
        total: Some(bytes.len() as u64),
    });
    context.check_cancelled()?;
    Ok(bytes)
}

#[derive(Debug)]
pub struct HeaderProbe {
    pub bytes: u64,
    pub header: BrdHeader,
}

pub fn probe_path(path: &Path, options: &ImportOptions) -> Result<HeaderProbe, ImportError> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    if bytes > options.max_file_bytes {
        return Err(ImportError::ResourceLimit {
            actual: bytes,
            limit: options.max_file_bytes,
        });
    }
    let mut prefix = vec![
        0;
        usize::try_from(bytes.min(HEADER_BYTES as u64)).map_err(|_| {
            ImportError::ResourceLimit {
                actual: bytes,
                limit: options.max_file_bytes,
            }
        })?
    ];
    file.read_exact(&mut prefix)?;
    Ok(HeaderProbe {
        bytes,
        header: BrdHeader::read(&prefix, options.text_encoding)?,
    })
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    #[test]
    fn fingerprint_matches_known_digest_and_observes_cancellation() {
        let token = pomelo_core::task::CancellationToken::default();
        let context = ImportContext {
            cancellation: &token,
            progress: &|_| {},
        };
        let digest = content_sha256(b"abc", &context).unwrap();
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(digest, content_sha256(b"abd", &context).unwrap());
        token.cancel();
        assert!(matches!(
            content_sha256(&[], &context),
            Err(ImportError::Cancelled)
        ));
    }
}
