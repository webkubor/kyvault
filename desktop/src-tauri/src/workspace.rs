//! One atomic file per UUID: a bounded encrypted header followed by encrypted content.
//! The header authenticates the id, kind, title and fresh content key for this revision.
//! Listing reads only the header; no separate index needs crash recovery.
use crate::commands::{active, DesktopState};
use anyhow::{anyhow, ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use fs2::FileExt;
use image::ImageDecoder;
use kyvault::crypto::{decrypt_joined, derive_file_key, encrypt_joined, new_master_key_b64};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, ErrorKind, Read, Write},
    path::Path,
};
use tauri::State;
use uuid::Uuid;

const MAX_BODY: usize = 1024 * 1024;
const MAX_PNG: usize = 1024 * 1024;
const MAX_ORIGINAL: usize = 8 * 1024 * 1024;
const MAX_HEADER: usize = 4096;
// Worst case JSON escaping for body, base64 original image, then encrypted envelope.
const MAX_CONTENT: usize = (6 * MAX_BODY + 4 * MAX_ORIGINAL.div_ceil(3) + 512).div_ceil(3) * 4;
const PNG_PREFIX: &str = "data:image/png;base64,";

#[derive(Copy, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Private,
    Drafts,
    Shares,
    Feedback,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    id: String,
    title: String,
    body: String,
    image: String,
}

#[derive(Serialize)]
pub struct Summary {
    id: String,
    title: String,
}

#[derive(Serialize, Deserialize)]
struct Header {
    id: String,
    kind: Kind,
    title: String,
    content_key: String,
}

#[derive(Serialize, Deserialize)]
struct Content {
    body: String,
    image: String,
}

// Copy only chunks needed to display the decoded PNG. This preserves the
// compressed pixels while removing EXIF, text, profiles and trailing data.
fn stripped_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = bytes.get(..8).ok_or(WorkspaceError("PNG 损坏"))?.to_vec();
    let mut rest = &bytes[8..];
    loop {
        let len = u32::from_be_bytes(
            rest.get(..4)
                .ok_or(WorkspaceError("PNG 损坏"))?
                .try_into()?,
        ) as usize;
        let size = len.checked_add(12).ok_or(WorkspaceError("PNG 损坏"))?;
        let chunk = rest.get(..size).ok_or(WorkspaceError("PNG 损坏"))?;
        let ty = &chunk[4..8];
        if matches!(ty, b"IHDR" | b"PLTE" | b"tRNS" | b"IDAT" | b"IEND") {
            out.extend_from_slice(chunk);
        }
        if ty == b"IEND" {
            return Ok(out);
        }
        rest = &rest[size..];
    }
}

fn validated_image(kind: Kind, image: String) -> Result<String> {
    if image.is_empty() {
        return Ok(image);
    }
    let original = matches!(kind, Kind::Private | Kind::Drafts);
    let max = if original { MAX_ORIGINAL } else { MAX_PNG };
    let (mime, encoded) = image
        .split_once(',')
        .ok_or(WorkspaceError("图片格式无效"))?;
    let format = match mime {
        "data:image/png;base64" => image::ImageFormat::Png,
        "data:image/jpeg;base64" if original => image::ImageFormat::Jpeg,
        "data:image/webp;base64" if original => image::ImageFormat::WebP,
        _ => return Err(WorkspaceError("不支持此图片格式").into()),
    };
    let too_large = if original {
        "原图超过 8 MiB"
    } else {
        "PNG 超过 1 MiB"
    };
    ensure!(
        encoded.len() <= max.div_ceil(3) * 4,
        WorkspaceError(too_large)
    );
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| WorkspaceError("图片编码无效"))?;
    ensure!(bytes.len() <= max, WorkspaceError(too_large));
    ensure!(
        image::guess_format(&bytes).ok() == Some(format),
        WorkspaceError("图片格式与 MIME 不一致")
    );
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(320 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| WorkspaceError("图片损坏或尺寸过大"))?;
    let (width, height) = decoder.dimensions();
    ensure!(
        width > 0
            && height > 0
            && width <= 16384
            && height <= 16384
            && u64::from(width) * u64::from(height) <= 40_000_000
            && decoder.total_bytes() <= 320 * 1024 * 1024,
        WorkspaceError("图片尺寸过大")
    );
    image::DynamicImage::from_decoder(decoder).map_err(|_| WorkspaceError("图片损坏"))?;
    if original {
        Ok(image)
    } else {
        Ok(format!(
            "{PNG_PREFIX}{}",
            STANDARD.encode(stripped_png(&bytes)?)
        ))
    }
}

fn exists_regular(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            ensure!(m.is_file(), "unexpected file type");
            Ok(true)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn read_limited(path: &Path, max: usize) -> Result<String> {
    let mut s = String::new();
    File::open(path)?
        .take(max as u64 + 1)
        .read_to_string(&mut s)?;
    ensure!(s.len() <= max, "file too large");
    Ok(s)
}

fn replace(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow!("missing parent"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

// The lock has a stable inode across atomic replacements, including key creation.
fn with_store<T>(root: &Path, operation: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
    ensure!(fs::symlink_metadata(root)?.is_dir(), "invalid data root");
    let dir = root.join("workspace");
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(&dir) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => (),
        result => result?,
    }
    ensure!(fs::symlink_metadata(&dir)?.is_dir(), "invalid workspace");
    let lock_path = dir.join(".lock");
    exists_regular(&lock_path)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let path = dir.join("workspace.key");
    if !exists_regular(&path)? {
        for entry in fs::read_dir(&dir)? {
            ensure!(
                entry?.path().extension().is_none_or(|e| e != "enc"),
                "workspace key missing"
            );
        }
        replace(&path, new_master_key_b64().as_bytes())?;
    }
    let raw = read_limited(&path, 44)?;
    ensure!(STANDARD.decode(&raw)?.len() == 32, "invalid key");
    operation(&Store {
        dir,
        key: derive_file_key(&raw)?,
    })
}

fn guarded<T>(state: &DesktopState, f: impl FnOnce(&Store) -> Result<T>) -> Result<T, String> {
    let _session = active(state)?;
    with_store(&state.data_root, f).map_err(|e| {
        e.downcast_ref::<WorkspaceError>()
            .map(|e| e.0.to_owned())
            .unwrap_or_else(|| "无法访问文档库，请检查密钥和文件状态".into())
    })
}

#[derive(Debug)]
struct WorkspaceError(&'static str);
impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for WorkspaceError {}

struct Store {
    dir: std::path::PathBuf,
    key: [u8; 32],
}

impl Store {
    fn path(&self, id: &str) -> Result<std::path::PathBuf> {
        let uuid = Uuid::parse_str(id).map_err(|_| WorkspaceError("文档 ID 无效"))?;
        ensure!(uuid.to_string() == id, WorkspaceError("文档 ID 无效"));
        Ok(self.dir.join(format!("{uuid}.enc")))
    }

    fn header(&self, id: &str) -> Result<(Header, File)> {
        let path = self.path(id)?;
        ensure!(exists_regular(&path)?, WorkspaceError("文档不存在"));
        let mut file = File::open(path)?;
        let mut len = [0; 4];
        file.read_exact(&mut len)?;
        let size = u32::from_be_bytes(len) as usize;
        ensure!(size <= MAX_HEADER, "invalid header length");
        let mut enc = vec![0; size];
        file.read_exact(&mut enc)?;
        let header: Header =
            serde_json::from_str(&decrypt_joined(std::str::from_utf8(&enc)?, &self.key)?)?;
        ensure!(header.id == id, "incorrect document id");
        Ok((header, file))
    }

    fn list(&self, kind: Kind) -> Result<Vec<Summary>> {
        let mut rows = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "enc") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow!("invalid id"))?;
            let (header, file) = self.header(id)?;
            if header.kind == kind {
                rows.push((
                    file.metadata()?.modified()?,
                    Summary {
                        id: header.id,
                        title: header.title,
                    },
                ));
            }
        }
        rows.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
        Ok(rows.into_iter().map(|(_, summary)| summary).collect())
    }

    fn read(&self, kind: Kind, id: &str) -> Result<Document> {
        let (header, file) = self.header(id)?;
        ensure!(header.kind == kind, WorkspaceError("文档不存在"));
        let mut encrypted = String::new();
        file.take(MAX_CONTENT as u64 + 1)
            .read_to_string(&mut encrypted)?;
        ensure!(encrypted.len() <= MAX_CONTENT, "content too large");
        let content: Content = serde_json::from_str(&decrypt_joined(
            &encrypted,
            &derive_file_key(&header.content_key)?,
        )?)?;
        Ok(Document {
            id: header.id,
            title: header.title,
            body: content.body,
            image: content.image,
        })
    }

    fn save(
        &self,
        kind: Kind,
        id: Option<String>,
        title: String,
        body: String,
        image: String,
    ) -> Result<String> {
        ensure!(
            !title.trim().is_empty() && title.len() <= 512 && !title.chars().any(char::is_control),
            WorkspaceError("标题无效或超过 512 字节")
        );
        ensure!(body.len() <= MAX_BODY, WorkspaceError("正文超过 1 MiB"));
        ensure!(
            !(kind == Kind::Shares && id.is_some()),
            WorkspaceError("已保存的分享是固定版本，请新建分享")
        );
        let image = validated_image(kind, image)?;
        let id = if let Some(id) = id {
            self.read(kind, &id)?;
            id
        } else {
            let id = Uuid::new_v4().to_string();
            ensure!(!exists_regular(&self.path(&id)?)?, "id collision");
            id
        };
        let key = new_master_key_b64();
        let content = encrypt_joined(
            &serde_json::to_string(&Content { body, image })?,
            &derive_file_key(&key)?,
        )?;
        let header = encrypt_joined(
            &serde_json::to_string(&Header {
                id: id.clone(),
                kind,
                title,
                content_key: key,
            })?,
            &self.key,
        )?;
        let mut data = Vec::with_capacity(4 + header.len() + content.len());
        data.extend_from_slice(&(header.len() as u32).to_be_bytes());
        data.extend_from_slice(header.as_bytes());
        data.extend_from_slice(content.as_bytes());
        replace(&self.path(&id)?, &data)?;
        Ok(id)
    }

    fn delete(&self, kind: Kind, id: &str) -> Result<()> {
        let (header, file) = self.header(id)?;
        ensure!(kind == header.kind, WorkspaceError("文档不存在"));
        drop(file);
        fs::remove_file(self.path(id)?)?;
        #[cfg(unix)]
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}

#[tauri::command(async)]
pub fn workspace_list(state: State<'_, DesktopState>, kind: Kind) -> Result<Vec<Summary>, String> {
    guarded(&state, |store| store.list(kind))
}
#[tauri::command(async)]
pub fn workspace_read(
    state: State<'_, DesktopState>,
    kind: Kind,
    id: String,
) -> Result<Document, String> {
    guarded(&state, |store| store.read(kind, &id))
}
#[tauri::command(async)]
pub fn workspace_save(
    state: State<'_, DesktopState>,
    kind: Kind,
    id: Option<String>,
    title: String,
    body: String,
    image: String,
) -> Result<String, String> {
    guarded(&state, |store| store.save(kind, id, title, body, image))
}
#[tauri::command(async)]
pub fn workspace_delete(
    state: State<'_, DesktopState>,
    kind: Kind,
    id: String,
) -> Result<(), String> {
    guarded(&state, |store| store.delete(kind, &id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Session;
    use kyvault::desktop::DesktopHistory;
    use std::sync::Mutex;

    fn state(root: &Path) -> DesktopState {
        DesktopState {
            history: DesktopHistory::new(root),
            data_root: root.into(),
            session: Mutex::new(Session::default()),
        }
    }

    fn save(
        store: &Store,
        kind: Kind,
        id: Option<String>,
        title: &str,
        body: &str,
    ) -> Result<String> {
        store.save(kind, id, title.into(), body.into(), String::new())
    }

    fn sample(format: image::ImageFormat) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut buf, format)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn separates_categories_and_checks_ids_before_file_access() {
        let root = tempfile::tempdir().unwrap();
        with_store(root.path(), |s| {
            let id = save(s, Kind::Private, None, "fixture title", "fixture body")?;
            let bytes = fs::read(s.path(&id)?)?;
            for value in ["fixture title", "fixture body"] {
                assert!(!bytes.windows(value.len()).any(|w| w == value.as_bytes()));
            }
            assert_eq!(s.read(Kind::Private, &id)?.body, "fixture body");
            assert!(s.list(Kind::Drafts)?.is_empty());
            assert!(s.delete(Kind::Shares, &id).is_err());
            assert!(save(s, Kind::Drafts, Some(id.clone()), "x", "y").is_err());
            assert!(s.read(Kind::Private, "../outside").is_err());
            assert_eq!(fs::read(s.path(&id)?)?, bytes);
            assert_eq!(
                save(s, Kind::Private, Some(id.clone()), "updated", "new")?,
                id
            );
            assert_eq!(s.list(Kind::Private)?[0].title, "updated");
            s.delete(Kind::Private, &id)?;
            assert!(s.list(Kind::Private)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn list_does_not_read_body_and_damaged_record_is_isolated() {
        let root = tempfile::tempdir().unwrap();
        with_store(root.path(), |s| {
            let first = save(s, Kind::Shares, None, "first", "secret")?;
            let other = save(s, Kind::Shares, None, "other", "independent")?;
            let path = s.path(&first)?;
            let bytes = fs::read(&path)?;
            let header_end = 4 + u32::from_be_bytes(bytes[..4].try_into()?) as usize;
            fs::write(&path, &bytes[..header_end])?;
            assert!(s.read(Kind::Shares, &first).is_err());
            assert!(save(s, Kind::Shares, Some(first), "bad", "bad").is_err());
            assert_eq!(s.list(Kind::Shares)?.len(), 2);
            assert_eq!(s.read(Kind::Shares, &other)?.body, "independent");
            Ok(())
        })
        .unwrap();
        fs::remove_file(root.path().join("workspace/workspace.key")).unwrap();
        assert!(with_store(root.path(), |_| Ok(())).is_err());
    }

    #[test]
    fn share_snapshot_rejects_update_and_preserves_original_file() {
        let root = tempfile::tempdir().unwrap();
        with_store(root.path(), |s| {
            let id = save(s, Kind::Shares, None, "snapshot title", "snapshot body")?;
            let path = s.path(&id)?;
            let before = fs::read(&path)?;
            assert!(save(s, Kind::Shares, Some(id.clone()), "edited", "replaced").is_err());
            assert_eq!(fs::read(path)?, before);
            let original = s.read(Kind::Shares, &id)?;
            assert_eq!(
                (original.title.as_str(), original.body.as_str()),
                ("snapshot title", "snapshot body")
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn parallel_sessions_commit_complete_revisions() {
        let root = tempfile::tempdir().unwrap();
        let id = with_store(root.path(), |s| {
            save(s, Kind::Drafts, None, "original", "original")
        })
        .unwrap();
        std::thread::scope(|scope| {
            for i in 0..8 {
                let root = root.path();
                let id = id.clone();
                scope.spawn(move || {
                    with_store(root, |s| {
                        let text = i.to_string();
                        save(s, Kind::Drafts, Some(id.clone()), &text, &text)?;
                        save(s, Kind::Feedback, None, &text, &text)?;
                        Ok(())
                    })
                    .unwrap()
                });
            }
        });
        with_store(root.path(), |s| {
            assert_eq!(s.list(Kind::Feedback)?.len(), 8);
            let doc = s.read(Kind::Drafts, &id)?;
            assert_eq!(doc.title, doc.body);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn private_images_roundtrip_without_reencoding() {
        let root = tempfile::tempdir().unwrap();
        with_store(root.path(), |s| {
            for (format, mime) in [
                (image::ImageFormat::Jpeg, "jpeg"),
                (image::ImageFormat::WebP, "webp"),
                (image::ImageFormat::Png, "png"),
            ] {
                let bytes = sample(format);
                let url = format!("data:image/{mime};base64,{}", STANDARD.encode(bytes));
                for kind in [Kind::Private, Kind::Drafts] {
                    let id = s.save(kind, None, "picture".into(), String::new(), url.clone())?;
                    assert_eq!(s.read(kind, &id)?.image, url);
                }
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn shares_strip_png_metadata_and_validate_type_and_size() {
        let mut png = sample(image::ImageFormat::Png);
        // Insert a valid tEXt chunk immediately after IHDR.
        let text = STANDARD
            .decode("AAAAEXRFWHRDb21tZW50AHNlY3JldDEyMz9adyQ=")
            .unwrap();
        png.splice(33..33, text);
        let orig = format!("{PNG_PREFIX}{}", STANDARD.encode(&png));
        assert_eq!(validated_image(Kind::Private, orig.clone()).unwrap(), orig);
        let shared = validated_image(Kind::Shares, orig).unwrap();
        let cleaned = STANDARD
            .decode(shared.strip_prefix(PNG_PREFIX).unwrap())
            .unwrap();
        assert!(!cleaned.windows(4).any(|w| w == b"tEXt"));
        assert!(image::load_from_memory(&cleaned).is_ok());
        assert!(validated_image(
            Kind::Drafts,
            format!("data:image/jpeg;base64,{}", STANDARD.encode(&png))
        )
        .is_err());
        assert!(validated_image(
            Kind::Shares,
            format!(
                "data:image/jpeg;base64,{}",
                STANDARD.encode(sample(image::ImageFormat::Jpeg))
            )
        )
        .is_err());
        assert!(validated_image(
            Kind::Private,
            format!(
                "{PNG_PREFIX}{}",
                "A".repeat(MAX_ORIGINAL.div_ceil(3) * 4 + 1)
            )
        )
        .is_err());
        assert!(validated_image(
            Kind::Shares,
            format!("{PNG_PREFIX}{}", "A".repeat(MAX_PNG.div_ceil(3) * 4 + 1))
        )
        .is_err());
    }

    #[test]
    fn rejects_oversized_dimensions_before_decoding_pixels() {
        for header in ["iVBORw0KGgoAAAANSUhEUgAAQAEAAAABCAIAAABGP0oxAAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC","iVBORw0KGgoAAAANSUhEUgAAH0AAABdwCAIAAAC+BnNpAAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC"] {
            let error=validated_image(Kind::Private, format!("{PNG_PREFIX}{header}")).unwrap_err();
            assert!(error.to_string().contains("尺寸过大"));
        }
    }

    #[test]
    fn library_can_exceed_old_total_size() {
        let root = tempfile::tempdir().unwrap();
        let body = "x".repeat(MAX_BODY);
        with_store(root.path(), |s| {
            for i in 0..25 {
                save(s, Kind::Drafts, None, &i.to_string(), &body)?;
            }
            assert_eq!(s.list(Kind::Drafts)?.len(), 25);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn locked_session_and_invalid_body_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        state.session.lock().unwrap().locked = true;
        assert!(guarded(&state, |s| s.list(Kind::Private)).is_err());
        assert!(guarded(&state, |s| save(s, Kind::Private, None, "locked", "x")).is_err());
        assert!(!root.path().join("workspace").exists());
        with_store(root.path(), |s| {
            assert!(save(s, Kind::Private, None, "", "x").is_err());
            assert!(save(s, Kind::Private, None, "title", &"a".repeat(MAX_BODY + 1)).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("workspace")).unwrap();
        assert!(with_store(root.path(), |_| Ok(())).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        fs::remove_file(root.path().join("workspace")).unwrap();
        with_store(root.path(), |s| {
            let id = Uuid::new_v4().to_string();
            symlink(outside.path(), s.path(&id)?)?;
            assert!(s.read(Kind::Private, &id).is_err());
            Ok(())
        })
        .unwrap();
    }
}
