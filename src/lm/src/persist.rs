//! Deterministic LM persistence (bead hearsay-four-2a0, R1b).
//!
//! Archives are compact JSON over the inspectable snapshot: `BTreeMap`
//! counts and a sorted vocabulary serialize in a fixed order, so
//! re-training on the same sentences and re-saving yields byte-identical
//! bytes. Reload rebuilds the model via `KnTrigram::from_inspectable`.
//! A `format`/`version` header is checked on load; unknown archives are
//! rejected instead of being silently misread.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::interpolate::InterpolatedLm;
use crate::kneser_ney::{InspectableModel, KnTrigram};

pub const ARCHIVE_FORMAT: &str = "hearsay-lm";
/// v2 drops the redundant `bigram_continuation_counts` map from archives
/// (recomputed exactly on load); v1 files are rejected, none shipped.
pub const ARCHIVE_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct KnArchive {
    format: String,
    version: u32,
    kind: String,
    model: InspectableModel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct InterpolatedArchive {
    format: String,
    version: u32,
    kind: String,
    personal_weight: f64,
    general: InspectableModel,
    personal: InspectableModel,
}

/// Header fields every LM archive carries, checked after a single-pass parse.
trait ArchiveKind {
    fn header(&self) -> (&str, u32, &str);
}

impl ArchiveKind for KnArchive {
    fn header(&self) -> (&str, u32, &str) {
        (&self.format, self.version, &self.kind)
    }
}

impl ArchiveKind for InterpolatedArchive {
    fn header(&self) -> (&str, u32, &str) {
        (&self.format, self.version, &self.kind)
    }
}

fn parse_archive<T: serde::de::DeserializeOwned + ArchiveKind>(
    bytes: &[u8],
    kind: &str,
) -> Result<T, String> {
    // Single pass straight into the typed struct: the old Value-then-convert
    // path parsed and walked multi-million-entry maps twice.
    let archive: T =
        serde_json::from_slice(bytes).map_err(|e| format!("parse LM archive: {e}"))?;
    let (format, version, actual) = archive.header();
    if format != ARCHIVE_FORMAT || version != ARCHIVE_VERSION || actual != kind {
        return Err(format!(
            "not a {ARCHIVE_FORMAT} v{ARCHIVE_VERSION} {kind} archive"
        ));
    }
    Ok(archive)
}

/// Serialize one KN model to deterministic archive bytes.
pub fn kn_to_bytes(model: &KnTrigram) -> Vec<u8> {
    let archive = KnArchive {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        kind: "kn-trigram".into(),
        model: model.inspectable(),
    };
    serde_json::to_vec(&archive).expect("serialize LM archive")
}

/// Rebuild a KN model from archive bytes.
pub fn kn_from_bytes(bytes: &[u8]) -> Result<KnTrigram, String> {
    let archive: KnArchive = parse_archive(bytes, "kn-trigram")?;
    KnTrigram::from_inspectable(&archive.model)
}

/// Save one KN model; reload with `load_kn`.
pub fn save_kn(model: &KnTrigram, path: &Path) -> Result<(), String> {
    std::fs::write(path, kn_to_bytes(model)).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Load a KN model saved by `save_kn`.
pub fn load_kn(path: &Path) -> Result<KnTrigram, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    kn_from_bytes(&bytes)
}

/// Serialize an interpolated LM (both components + weight) deterministically.
pub fn interpolated_to_bytes(lm: &InterpolatedLm) -> Vec<u8> {
    let archive = InterpolatedArchive {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        kind: "interpolated".into(),
        personal_weight: lm.personal_weight(),
        general: lm.general().inspectable(),
        personal: lm.personal().inspectable(),
    };
    serde_json::to_vec(&archive).expect("serialize LM archive")
}

/// Rebuild an interpolated LM from archive bytes.
pub fn interpolated_from_bytes(bytes: &[u8]) -> Result<InterpolatedLm, String> {
    let archive: InterpolatedArchive = parse_archive(bytes, "interpolated")?;
    InterpolatedLm::new(
        KnTrigram::from_inspectable(&archive.general)?,
        KnTrigram::from_inspectable(&archive.personal)?,
        archive.personal_weight,
    )
}

/// Save an interpolated LM; reload with `load_interpolated`.
pub fn save_interpolated(lm: &InterpolatedLm, path: &Path) -> Result<(), String> {
    std::fs::write(path, interpolated_to_bytes(lm))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// Load an interpolated LM saved by `save_interpolated`.
pub fn load_interpolated(path: &Path) -> Result<InterpolatedLm, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    interpolated_from_bytes(&bytes)
}
