//! Whole-library backup: the library folder as one zip file.
//!
//! The zip writer/reader is hand-rolled on purpose — stored
//! (uncompressed) entries only, which keeps the format trivially
//! auditable and adds no dependency. A library is documents and
//! images; it compresses well in the user's own archiver later if
//! they care, and a backup that any tool can open beats a smaller
//! one that needs this app.
//!
//! Restore is deliberately non-destructive: extraction skips every
//! file that already exists, so restoring an old backup brings back
//! what was lost and never overwrites what is there now.

use std::io;
use std::path::{Path, PathBuf};

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const END_SIG: u32 = 0x0605_4b50;
const UTF8_FLAG: u16 = 0x0800;

/// Files that are runtime state, not library content.
fn skipped(name: &str) -> bool {
    name == "agent-bridge.json" || name == ".lock" || name.ends_with(".tmp")
}

/// Every file under `root`, as (zip name, path), sorted by name so a
/// backup's layout is deterministic.
fn collect(root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                let name = rel.to_string_lossy().replace('\\', "/");
                if !skipped(&name) { out.push((name, path)); }
            }
        }
    }
    out.sort();
    Ok(out)
}

// CRC-32 (IEEE), table built on first use.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 { c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 }; }
            *slot = c;
        }
        t
    });
    let mut crc = !0u32;
    for &b in data { crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8); }
    !crc
}

/// DOS date/time for a file's modification time, in UTC.
fn dos_time(modified: std::time::SystemTime) -> (u16, u16) {
    let secs = modified.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let day_secs = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let date = (((year - 1980).max(0) as u16) << 9) | ((month as u16) << 5) | day as u16;
    let time = (((day_secs / 3600) as u16) << 11) | (((day_secs % 3600 / 60) as u16) << 5) | ((day_secs % 60 / 2) as u16);
    (date, time)
}

fn put16(buf: &mut Vec<u8>, v: u16) { buf.extend_from_slice(&v.to_le_bytes()); }
fn put32(buf: &mut Vec<u8>, v: u32) { buf.extend_from_slice(&v.to_le_bytes()); }

/// Write every library file under `root` into a zip at `out`.
/// Returns the number of files written.
pub fn write_backup(root: &Path, out: &Path) -> io::Result<usize> {
    let files = collect(root)?;
    let mut zip: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, path) in &files {
        let data = std::fs::read(path)?;
        let meta = std::fs::metadata(path)?;
        let (date, time) = dos_time(meta.modified().unwrap_or(std::time::UNIX_EPOCH));
        let crc = crc32(&data);
        let offset = zip.len() as u32;
        put32(&mut zip, LOCAL_SIG);
        put16(&mut zip, 20);
        put16(&mut zip, UTF8_FLAG);
        put16(&mut zip, 0); // stored
        put16(&mut zip, time);
        put16(&mut zip, date);
        put32(&mut zip, crc);
        put32(&mut zip, data.len() as u32);
        put32(&mut zip, data.len() as u32);
        put16(&mut zip, name.len() as u16);
        put16(&mut zip, 0);
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(&data);
        put32(&mut central, CENTRAL_SIG);
        put16(&mut central, 20);
        put16(&mut central, 20);
        put16(&mut central, UTF8_FLAG);
        put16(&mut central, 0);
        put16(&mut central, time);
        put16(&mut central, date);
        put32(&mut central, crc);
        put32(&mut central, data.len() as u32);
        put32(&mut central, data.len() as u32);
        put16(&mut central, name.len() as u16);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = zip.len() as u32;
    let count = files.len() as u16;
    zip.extend_from_slice(&central);
    put32(&mut zip, END_SIG);
    put16(&mut zip, 0);
    put16(&mut zip, 0);
    put16(&mut zip, count);
    put16(&mut zip, count);
    put32(&mut zip, central.len() as u32);
    put32(&mut zip, central_offset);
    put16(&mut zip, 0);
    std::fs::write(out, zip)?;
    Ok(files.len())
}

struct Entry {
    name: String,
    method: u16,
    size: usize,
    offset: usize,
    crc: u32,
}

fn get16(b: &[u8], at: usize) -> io::Result<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| bad("truncated zip"))
}
fn get32(b: &[u8], at: usize) -> io::Result<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| bad("truncated zip"))
}
fn bad(msg: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, msg.to_string()) }

/// Parse a zip's central directory. Only stored entries are
/// supported (that is all `write_backup` produces).
fn entries(zip: &[u8]) -> io::Result<Vec<Entry>> {
    // The end record sits in the last 64 KiB (comment included).
    let tail = zip.len().saturating_sub(65_557);
    let end = (tail..zip.len().saturating_sub(3))
        .rev()
        .find(|&i| get32(zip, i).ok() == Some(END_SIG))
        .ok_or_else(|| bad("not a zip file"))?;
    let count = get16(zip, end + 10)? as usize;
    let mut at = get32(zip, end + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if get32(zip, at)? != CENTRAL_SIG { return Err(bad("corrupt zip directory")); }
        let name_len = get16(zip, at + 28)? as usize;
        let extra_len = get16(zip, at + 30)? as usize;
        let comment_len = get16(zip, at + 32)? as usize;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(|| bad("truncated zip"))?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: get16(zip, at + 10)?,
            size: get32(zip, at + 20)? as usize,
            crc: get32(zip, at + 16)?,
            offset: get32(zip, at + 42)? as usize,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// The entry names in a zip — enough to check it is one of ours.
pub fn entry_names(zip: &[u8]) -> io::Result<Vec<String>> {
    Ok(entries(zip)?.into_iter().map(|e| e.name).collect())
}

/// A backup of this app's library carries both markers at its root.
pub fn looks_like_library(names: &[String]) -> bool {
    names.iter().any(|n| n == "format.json") && names.iter().any(|n| n == "index.json")
}

/// A zip entry name that is safe to write under a folder: relative,
/// forward slashes, no parent hops, no drive or root prefix.
fn safe_name(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\\') || name.starts_with('/') || name.contains(':') { return None; }
    let mut out = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." { continue; }
        if part == ".." { return None; }
        out.push(part);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Extract a backup under `dest`, skipping files that already exist
/// (restore never overwrites) and refusing unsafe names. Returns
/// (written, skipped).
pub fn extract_backup(zip: &[u8], dest: &Path) -> io::Result<(usize, usize)> {
    let (mut written, mut skipped) = (0usize, 0usize);
    for entry in entries(zip)? {
        if entry.name.ends_with('/') { continue; }
        if entry.method != 0 { return Err(bad("this backup uses compression this app cannot read")); }
        let Some(rel) = safe_name(&entry.name) else { return Err(bad("this backup contains an unsafe file path")) };
        let target = dest.join(&rel);
        if target.exists() { skipped += 1; continue; }
        // Local header: name/extra lengths may differ from central.
        let at = entry.offset;
        if get32(zip, at)? != LOCAL_SIG { return Err(bad("corrupt zip entry")); }
        let start = at + 30 + get16(zip, at + 26)? as usize + get16(zip, at + 28)? as usize;
        let data = zip.get(start..start + entry.size).ok_or_else(|| bad("truncated zip entry"))?;
        if crc32(data) != entry.crc { return Err(bad("a file in this backup is damaged")); }
        if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(&target, data)?;
        written += 1;
    }
    Ok((written, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aimd-backup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn backup_roundtrips_and_restore_never_overwrites() {
        let root = tempdir("root");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("format.json"), "{}").unwrap();
        std::fs::write(root.join("index.json"), "{}").unwrap();
        std::fs::write(root.join("docs/a.xml"), "<doc/>").unwrap();
        std::fs::write(root.join("agent-bridge.json"), "runtime").unwrap();
        let out = tempdir("out").join("b.zip");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        assert_eq!(write_backup(&root, &out).unwrap(), 3);
        let zip = std::fs::read(&out).unwrap();
        let names = entry_names(&zip).unwrap();
        assert!(looks_like_library(&names));
        assert!(!names.iter().any(|n| n.contains("agent-bridge")));
        let dest = tempdir("dest");
        let (written, skipped) = extract_backup(&zip, &dest).unwrap();
        assert_eq!((written, skipped), (3, 0));
        assert_eq!(std::fs::read_to_string(dest.join("docs/a.xml")).unwrap(), "<doc/>");
        // Second restore: everything exists, nothing is overwritten.
        std::fs::write(dest.join("docs/a.xml"), "edited").unwrap();
        let (written, skipped) = extract_backup(&zip, &dest).unwrap();
        assert_eq!((written, skipped), (0, 3));
        assert_eq!(std::fs::read_to_string(dest.join("docs/a.xml")).unwrap(), "edited");
    }

    #[test]
    fn unsafe_entry_names_are_refused() {
        assert!(safe_name("../evil").is_none());
        assert!(safe_name("docs/../../evil").is_none());
        assert!(safe_name("/abs/path").is_none());
        assert!(safe_name("C:/evil").is_none());
        assert!(safe_name("docs\\evil").is_none());
        assert_eq!(safe_name("docs/a.xml"), Some(PathBuf::from("docs/a.xml")));
    }
}
