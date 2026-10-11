//! PDFs saved by Revise carry their editable document as an embedded
//! attachment (a "hybrid PDF"), so reopening restores tables, lists and
//! styles exactly instead of reconstructing them. The attachment records a
//! fingerprint of the PDF's text; if another program edits the PDF, the
//! fingerprint no longer matches and the file is reconstructed as usual.

use document::Document;
use layout::DocLayout;
use pdf_source::PdfSource;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

const ATTACHMENT: &str = "revise-document.json";
/// Attachment name used before the app was renamed from Reflow.
const LEGACY_ATTACHMENT: &str = "reflow-document.json";

/// `~/Library/Application Support/Revise/<sub>` (created on demand; data
/// from before the rename is moved over from `.../Reflow`).
pub fn support_dir(sub: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let root = PathBuf::from(home).join("Library/Application Support");
    if !root.join("Revise").exists() && root.join("Reflow").exists() {
        let _ = std::fs::rename(root.join("Reflow"), root.join("Revise"));
    }
    let dir = root.join("Revise").join(sub);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Stable per-file key: FNV-1a of the absolute path.
fn key(path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in abs.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Where the untouched original of `path` is kept once Revise overwrites it.
pub fn original_backup(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(support_dir("Originals")?.join(format!("{}-{name}", key(path))))
}

pub fn has_original_backup(path: &Path) -> bool {
    original_backup(path).is_some_and(|b| b.exists())
}

/// Before the first overwrite of an existing file, keep a copy of it. Later
/// saves leave that copy alone, so it is always the pre-Revise version.
pub fn ensure_original_backup(path: &Path) -> std::io::Result<()> {
    let Some(backup) = original_backup(path) else { return Ok(()) };
    if path.exists() && !backup.exists() {
        std::fs::copy(path, backup)?;
    }
    Ok(())
}

pub fn restore_original(path: &Path) -> std::io::Result<()> {
    let backup = original_backup(path).ok_or_else(|| std::io::Error::other("no backup location"))?;
    std::fs::copy(backup, path).map(|_| ())
}

fn autosave_file(path: &Path) -> Option<PathBuf> {
    Some(support_dir("Autosave")?.join(format!("{}.json", key(path))))
}

/// Periodic crash-safety copy of unsaved edits to `path`.
pub fn write_autosave(path: &Path, doc: &Document) {
    if let (Some(file), Ok(bytes)) = (autosave_file(path), doc.to_bytes(None)) {
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }
}

/// Unsaved edits left behind (e.g. by a crash) that are newer than the file.
pub fn read_autosave(path: &Path) -> Option<(Document, SystemTime)> {
    let file = autosave_file(path)?;
    let saved = std::fs::metadata(&file).ok()?.modified().ok()?;
    let current = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    if current.is_some_and(|c| c >= saved) {
        clear_autosave(path);
        return None;
    }
    Some((Document::from_bytes(&std::fs::read(file).ok()?).ok()?.0, saved))
}

pub fn clear_autosave(path: &Path) {
    if let Some(file) = autosave_file(path) {
        let _ = std::fs::remove_file(file);
    }
}

pub fn pdf_bytes(doc: &Document, layout: &DocLayout) -> Result<Vec<u8>, String> {
    // Fingerprint exactly what PDFium will extract from the saved file, so
    // the check on reopening compares like with like.
    let plain = export::export_pdf(layout)?;
    let fingerprint = PdfSource::from_bytes(plain)?.text_fingerprint()?;
    let json = doc.to_bytes(Some(fingerprint)).map_err(|e| e.to_string())?;
    export::export_pdf_with_attachment(layout, Some((ATTACHMENT, &json)))
}

/// The embedded editable document, if present and still matching the PDF.
pub fn embedded_document(source: &PdfSource) -> Option<Document> {
    let (doc, fingerprint) = Document::from_bytes(&source.attachment(ATTACHMENT).or_else(|| source.attachment(LEGACY_ATTACHMENT))?).ok()?;
    let matches = fingerprint? == source.text_fingerprint().ok()? && doc.sections.len() == source.page_count() as usize;
    matches.then_some(doc)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn fixture_doc() -> Option<Document> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/resume-single.pdf");
        let src = PdfSource::open(&path).ok()?;
        Some(Document { sections: vec![Arc::new(reconstruction::import_page(&src, 0).ok()?.0)] })
    }

    /// Backups and autosaves live under $HOME; point it at a temp dir.
    #[test]
    fn original_backup_and_autosave_lifecycle() {
        let home = std::env::temp_dir().join(format!("revise-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: only this test reads HOME in this process's persist code paths.
        unsafe { std::env::set_var("HOME", &home) };
        let file = home.join("resume.pdf");
        std::fs::write(&file, b"original").unwrap();

        ensure_original_backup(&file).unwrap();
        std::fs::write(&file, b"first save").unwrap();
        ensure_original_backup(&file).unwrap(); // must not replace the original copy
        std::fs::write(&file, b"second save").unwrap();
        assert!(has_original_backup(&file));
        restore_original(&file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"original");

        // An autosave newer than the file is offered; one older is dropped.
        let doc = Document::default();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_autosave(&file, &doc);
        assert!(read_autosave(&file).is_some());
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, b"saved after the autosave").unwrap();
        assert!(read_autosave(&file).is_none());
        clear_autosave(&file);
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn saved_pdf_restores_the_exact_document() {
        let Some(doc) = fixture_doc() else { return };
        let mut e = editor::Editor::new(doc);
        e.set_caret(editor::Pos::new(3, 10));
        e.insert_table(2, 2, 300.0);
        e.insert_text("kept");
        let layout = layout::Layouter::new().layout(&e.doc);
        let saved = PdfSource::from_bytes(pdf_bytes(&e.doc, &layout).unwrap()).unwrap();
        let restored = embedded_document(&saved).expect("embedded document restored");
        assert_eq!(restored.outline(), e.doc.outline());
    }

    #[test]
    fn pdfs_saved_under_the_old_name_still_restore() {
        let Some(doc) = fixture_doc() else { return };
        let layout = layout::Layouter::new().layout(&doc);
        let fingerprint = PdfSource::from_bytes(export::export_pdf(&layout).unwrap()).unwrap().text_fingerprint().unwrap();
        let json = String::from_utf8(doc.to_bytes(Some(fingerprint)).unwrap()).unwrap().replacen(r#""format":"revise""#, r#""format":"reflow""#, 1);
        let legacy = export::export_pdf_with_attachment(&layout, Some((LEGACY_ATTACHMENT, json.as_bytes()))).unwrap();
        assert!(embedded_document(&PdfSource::from_bytes(legacy).unwrap()).is_some());
    }

    #[test]
    fn pdf_changed_elsewhere_is_reconstructed_instead() {
        let Some(doc) = fixture_doc() else { return };
        let layout = layout::Layouter::new().layout(&doc);
        let original = pdf_bytes(&doc, &layout).unwrap();
        let fingerprint = PdfSource::from_bytes(original).unwrap().text_fingerprint().unwrap();

        // Same attachment, different page text: as if another app edited it.
        let mut e = editor::Editor::new(doc.clone());
        e.set_caret(editor::Pos::new(0, 0));
        e.insert_text("Edited elsewhere ");
        let other = layout::Layouter::new().layout(&e.doc);
        let json = doc.to_bytes(Some(fingerprint)).unwrap();
        let tampered = export::export_pdf_with_attachment(&other, Some((ATTACHMENT, &json))).unwrap();
        assert!(embedded_document(&PdfSource::from_bytes(tampered).unwrap()).is_none());
    }
}
