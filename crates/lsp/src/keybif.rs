//! KEY/BIF file reader for extracting resources from NWN:EE game data.
//!
//! Supports both V1 (original NWN) and E1 (Enhanced Edition) formats.
//! E1 adds optional zstd/zlib compression via the CompressedBuf wrapper.

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// NWScript source file resource type.
const RESTYPE_NSS: u16 = 2009;

/// A resource extracted from KEY/BIF files.
pub struct ExtractedResource {
    pub resref: String,
    pub data: Vec<u8>,
}

/// Extract all .nss resources from KEY/BIF files in an NWN installation.
///
/// Reads all .key files in `<nwn_root>/data/`, resolves their BIF references,
/// and extracts every resource with ResType 2009 (.nss).
///
/// Later KEY files override earlier ones (matching NWN's load order).
pub fn extract_nss_from_install(nwn_root: &Path) -> Result<Vec<ExtractedResource>, String> {
    let data_dir = nwn_root.join("data");
    if !data_dir.is_dir() {
        return Err(format!("NWN data directory not found: {}", data_dir.display()));
    }

    // NWN loads keys in this order (lowest to highest priority)
    let key_names = ["nwn_base.key", "nwn_base_loc.key", "nwn_retail.key", "nwn_retail_loc.key"];

    // Collect all .nss resources, later keys override earlier ones
    let mut resources: HashMap<String, ExtractedResource> = HashMap::new();

    for key_name in &key_names {
        let key_path = data_dir.join(key_name);
        if !key_path.exists() {
            continue;
        }

        match extract_nss_from_key(&key_path, nwn_root) {
            Ok(entries) => {
                tracing::info!(
                    "extracted {} .nss resources from {}",
                    entries.len(),
                    key_name
                );
                for entry in entries {
                    resources.insert(entry.resref.clone(), entry);
                }
            }
            Err(e) => {
                tracing::warn!("failed to read {}: {}", key_name, e);
            }
        }
    }

    Ok(resources.into_values().collect())
}

// =============================================================================
// KEY file parsing
// =============================================================================

struct KeyHeader {
    is_e1: bool,
    bif_count: u32,
    key_count: u32,
    offset_to_file_table: u32,
    offset_to_key_table: u32,
}

struct KeyFileEntry {
    filename: String,
}

struct KeyResEntry {
    resref: String,
    res_id: u32,
}

fn extract_nss_from_key(
    key_path: &Path,
    nwn_root: &Path,
) -> Result<Vec<ExtractedResource>, String> {
    let data = std::fs::read(key_path).map_err(|e| format!("read error: {e}"))?;
    let mut cur = Cursor::new(&data);

    let header = read_key_header(&mut cur)?;

    // Read file table (BIF filenames)
    let bif_entries = read_key_file_table(&mut cur, &data, &header)?;

    // Read key table (resource entries), filter to .nss only
    let nss_entries = read_key_table_nss(&mut cur, &header)?;

    // Group entries by BIF index
    let mut by_bif: HashMap<u32, Vec<&KeyResEntry>> = HashMap::new();
    for entry in &nss_entries {
        let bif_idx = entry.res_id >> 20;
        by_bif.entry(bif_idx).or_default().push(entry);
    }

    // Extract from each referenced BIF
    let mut results = Vec::new();
    for (bif_idx, entries) in &by_bif {
        let bif_idx = *bif_idx as usize;
        if bif_idx >= bif_entries.len() {
            tracing::warn!("BIF index {} out of range", bif_idx);
            continue;
        }

        // BIF filenames use backslashes in the KEY file, normalize to OS path
        let bif_rel = bif_entries[bif_idx].filename.replace('\\', "/");
        let bif_path = nwn_root.join(&bif_rel);
        if !bif_path.exists() {
            tracing::warn!("BIF not found: {}", bif_path.display());
            continue;
        }

        let bif_data = match std::fs::read(&bif_path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("failed to read BIF {}: {}", bif_path.display(), e);
                continue;
            }
        };

        for entry in entries {
            let var_idx = entry.res_id & 0xFFFFF;
            match extract_from_bif(&bif_data, var_idx) {
                Ok(resource_data) => {
                    results.push(ExtractedResource {
                        resref: entry.resref.clone(),
                        data: resource_data,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to extract {} from BIF: {}",
                        entry.resref,
                        e
                    );
                }
            }
        }
    }

    Ok(results)
}

fn read_key_header(cur: &mut Cursor<&Vec<u8>>) -> Result<KeyHeader, String> {
    let file_type = read_bytes(cur, 4)?;
    let file_type_str = String::from_utf8_lossy(&file_type);
    if file_type_str != "KEY " {
        return Err(format!("not a KEY file (got {:?})", file_type_str));
    }

    let version = read_bytes(cur, 4)?;
    let version_str = String::from_utf8_lossy(&version);
    let is_e1 = match version_str.as_ref() {
        "V1  " => false,
        "E1  " => true,
        _ => return Err(format!("unsupported KEY version: {:?}", version_str)),
    };

    let bif_count = read_u32(cur)?;
    let key_count = read_u32(cur)?;
    let offset_to_file_table = read_u32(cur)?;
    let offset_to_key_table = read_u32(cur)?;

    Ok(KeyHeader {
        is_e1,
        bif_count,
        key_count,
        offset_to_file_table,
        offset_to_key_table,
    })
}

fn read_key_file_table(
    cur: &mut Cursor<&Vec<u8>>,
    data: &[u8],
    header: &KeyHeader,
) -> Result<Vec<KeyFileEntry>, String> {
    cur.seek(SeekFrom::Start(header.offset_to_file_table as u64))
        .map_err(|e| format!("seek error: {e}"))?;

    let mut entries = Vec::with_capacity(header.bif_count as usize);
    for _ in 0..header.bif_count {
        let _file_size = read_u32(cur)?;
        let filename_offset = read_u32(cur)?;
        let filename_size = read_u16(cur)?;
        let _drives = read_u16(cur)?;

        let start = filename_offset as usize;
        let end = start + filename_size as usize;
        if end > data.len() {
            return Err("filename offset out of bounds".into());
        }

        let raw = &data[start..end];
        // Trim any null bytes from the filename
        let filename = String::from_utf8_lossy(raw)
            .trim_end_matches('\0')
            .to_string();

        entries.push(KeyFileEntry { filename });
    }

    Ok(entries)
}

fn read_key_table_nss(
    cur: &mut Cursor<&Vec<u8>>,
    header: &KeyHeader,
) -> Result<Vec<KeyResEntry>, String> {
    cur.seek(SeekFrom::Start(header.offset_to_key_table as u64))
        .map_err(|e| format!("seek error: {e}"))?;

    let entry_size: u64 = if header.is_e1 { 42 } else { 22 };
    let mut entries = Vec::new();

    for i in 0..header.key_count {
        let entry_start = header.offset_to_key_table as u64 + i as u64 * entry_size;
        cur.seek(SeekFrom::Start(entry_start))
            .map_err(|e| format!("seek error: {e}"))?;

        let resref_bytes = read_bytes(cur, 16)?;
        let resref = String::from_utf8_lossy(&resref_bytes)
            .trim_end_matches('\0')
            .to_lowercase();

        let restype = read_u16(cur)?;
        let res_id = read_u32(cur)?;

        // Only collect .nss resources
        if restype == RESTYPE_NSS {
            entries.push(KeyResEntry {
                resref,
                res_id,
            });
        }
    }

    Ok(entries)
}

// =============================================================================
// BIF file parsing
// =============================================================================

fn extract_from_bif(bif_data: &[u8], var_idx: u32) -> Result<Vec<u8>, String> {
    let mut cur = Cursor::new(bif_data);

    // Read BIF header
    let file_type = read_bytes(&mut cur, 4)?;
    if String::from_utf8_lossy(&file_type) != "BIFF" {
        return Err("not a BIF file".into());
    }

    let version = read_bytes(&mut cur, 4)?;
    let version_str = String::from_utf8_lossy(&version);
    let is_e1 = match version_str.as_ref() {
        "V1  " => false,
        "E1  " => true,
        _ => return Err(format!("unsupported BIF version: {:?}", version_str)),
    };

    let var_res_count = read_u32(&mut cur)?;
    let _fixed_res_count = read_u32(&mut cur)?;
    let var_table_offset = read_u32(&mut cur)?;

    if var_idx >= var_res_count {
        return Err(format!(
            "variable resource index {} >= count {}",
            var_idx, var_res_count
        ));
    }

    // Seek to the variable resource entry
    let entry_size: u64 = if is_e1 { 24 } else { 16 };
    let entry_offset = var_table_offset as u64 + var_idx as u64 * entry_size;
    cur.seek(SeekFrom::Start(entry_offset))
        .map_err(|e| format!("seek error: {e}"))?;

    let _id = read_u32(&mut cur)?;
    let data_offset = read_u32(&mut cur)?;
    let data_size = read_u32(&mut cur)?;
    let _res_type = read_u32(&mut cur)?;

    let (compression_type, _uncompressed_size) = if is_e1 {
        (read_u32(&mut cur)?, read_u32(&mut cur)?)
    } else {
        (0, 0)
    };

    // Read raw resource data
    let start = data_offset as usize;
    let end = start + data_size as usize;
    if end > bif_data.len() {
        return Err("resource data offset out of bounds".into());
    }
    let raw_data = &bif_data[start..end];

    // Decompress if needed
    if compression_type == 1 {
        decompress_compressed_buf(raw_data)
    } else {
        Ok(raw_data.to_vec())
    }
}

// =============================================================================
// CompressedBuf decompression (E1 format)
// =============================================================================

fn decompress_compressed_buf(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 16 {
        return Err("CompressedBuf too short for header".into());
    }

    let mut cur = Cursor::new(data);

    let magic = read_u32(&mut cur)?;
    if magic != 0x53455258 {
        // "XRES" in little-endian
        return Err(format!("bad CompressedBuf magic: 0x{:08X}", magic));
    }

    let header_version = read_u32(&mut cur)?;
    if header_version != 3 {
        return Err(format!(
            "unsupported CompressedBuf version: {}",
            header_version
        ));
    }

    let algorithm = read_u32(&mut cur)?;
    let uncompressed_size = read_u32(&mut cur)? as usize;

    match algorithm {
        0 => {
            // No compression — remaining data is the resource
            let pos = cur.position() as usize;
            Ok(data[pos..].to_vec())
        }
        1 => {
            // Zlib
            let pos = cur.position() as usize;
            let compressed = &data[pos..];
            let mut decoder = flate2::read::ZlibDecoder::new(compressed);
            let mut result = Vec::with_capacity(uncompressed_size);
            decoder
                .read_to_end(&mut result)
                .map_err(|e| format!("zlib decompression failed: {e}"))?;
            Ok(result)
        }
        2 => {
            // Zstd — has additional sub-header
            let _zstd_version = read_u32(&mut cur)?;
            let _dictionary_id = read_u32(&mut cur)?;
            let pos = cur.position() as usize;
            let compressed = &data[pos..];
            zstd::stream::decode_all(compressed)
                .map_err(|e| format!("zstd decompression failed: {e}"))
        }
        _ => Err(format!("unknown compression algorithm: {}", algorithm)),
    }
}

// =============================================================================
// Binary reading helpers
// =============================================================================

fn read_bytes(cur: &mut Cursor<impl AsRef<[u8]>>, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    cur.read_exact(&mut buf)
        .map_err(|e| format!("read error: {e}"))?;
    Ok(buf)
}

fn read_u32(cur: &mut Cursor<impl AsRef<[u8]>>) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)
        .map_err(|e| format!("read u32: {e}"))?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u16(cur: &mut Cursor<impl AsRef<[u8]>>) -> Result<u16, String> {
    let mut buf = [0u8; 2];
    cur.read_exact(&mut buf)
        .map_err(|e| format!("read u16: {e}"))?;
    Ok(u16::from_le_bytes(buf))
}
