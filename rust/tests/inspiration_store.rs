use kyvault::inspiration::InspirationStore;
use std::fs;

#[test]
fn markdown_roundtrips_unicode_source_and_stable_readable_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let store = InspirationStore::new(tmp.path());
    let body = "第一段\n\n# 正文标题\n\n末段\n\n";
    let a = store
        .add_with_source(
            "想法 🌱\n第二行",
            body,
            vec!["标签 --> 🌱".into()],
            Some("https://example.org/a?x=-->好"),
        )
        .unwrap();
    let b = store.add("想法 🌱\n第二行", "不同正文", vec![]).unwrap();
    assert_ne!(a.path, b.path);
    assert!(a.path.ends_with("想法 🌱 第二行.md"));
    let raw = fs::read_to_string(&a.path).unwrap();
    assert!(raw.contains("# 想法 🌱 第二行\n\n第一段"));
    assert_eq!(raw.matches(" -->").count(), 1);
    assert_eq!(store.get(&a.id).unwrap(), a);
    let updated = store
        .update(&a.id, Some("新标题".into()), None, None)
        .unwrap();
    assert_eq!(updated.path, a.path);
    assert_eq!(updated.body, body);
    assert_eq!(updated.source, a.source);
    assert_eq!(updated.tags, a.tags);
}

#[test]
fn external_body_edits_preserve_all_paragraphs_and_unknown_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let store = InspirationStore::new(tmp.path());
    let idea = store.add("原题", "原文", vec![]).unwrap();
    let raw = fs::read_to_string(&idea.path).unwrap();
    let (metadata, _) = raw.split_once('\n').unwrap();
    let metadata = metadata.replace("\"id\":", "\"unknown\":{\"keep\":true},\"id\":");
    let body = "首段\n\n---\n\n# 文内标题\n\n末段\n\n";
    fs::write(&idea.path, format!("{metadata}\n# 外部标题\n\n{body}")).unwrap();
    let reread = store.get(&idea.id).unwrap();
    assert_eq!(reread.title, "外部标题");
    assert_eq!(reread.body, body);
    let changed = store
        .update(&idea.id, None, None, Some(vec!["外部".into()]))
        .unwrap();
    assert_eq!(changed.body, body);
    let saved = fs::read_to_string(&idea.path).unwrap();
    assert!(saved.contains("\"unknown\":{\"keep\":true}"));
    assert!(saved.ends_with(body));
}

#[test]
fn unmanaged_notes_keep_frontmatter_and_prefix_on_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("外部.md");
    let prefix = "---\ncustom: keep\n---\n\n标题前首段\n\n";
    let body = "正文首段\n\n正文末段\n\n";
    fs::write(&path, format!("{prefix}# 外部标题\n\n{body}")).unwrap();
    let store = InspirationStore::new(tmp.path());
    let idea = store.list().unwrap().pop().unwrap();
    assert_eq!(idea.title, "外部标题");
    assert_eq!(idea.body, body);
    fs::write(&path, format!("{prefix}# 外部标题\n\n{body}追加")).unwrap();
    assert_eq!(store.list().unwrap()[0].id, idea.id);
    store
        .update(&idea.id, Some("改名".into()), None, None)
        .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains(prefix));
    assert!(raw.ends_with(&format!("{body}追加")));
    fs::rename(&path, tmp.path().join("重命名.md")).unwrap();
    assert_eq!(store.get(&idea.id).unwrap().title, "改名");
}

#[test]
fn unmanaged_without_heading_preserves_the_entire_body() {
    let tmp = tempfile::tempdir().unwrap();
    let body = "第一段\n\n最后一段\n\n";
    fs::write(tmp.path().join("文件名.md"), body).unwrap();
    let store = InspirationStore::new(tmp.path());
    let idea = store.list().unwrap().pop().unwrap();
    assert_eq!(idea.title, "文件名");
    assert_eq!(idea.body, body);
    let updated = store.update(&idea.id, None, None, None).unwrap();
    assert_eq!(store.get(&idea.id).unwrap(), updated);
    assert_eq!(updated.body, body);
}

#[test]
fn images_use_relative_markdown_links() {
    let tmp = tempfile::tempdir().unwrap();
    let store = InspirationStore::new(tmp.path());
    let png = b"\x89PNG\r\n\x1a\nfixture";
    let idea = store
        .capture_image(png, "图片灵感", Some("截图来源"))
        .unwrap();
    let relative = idea
        .body
        .trim()
        .strip_prefix("![图片](")
        .unwrap()
        .strip_suffix(')')
        .unwrap();
    assert!(relative.starts_with("附件/"));
    assert_eq!(fs::read(tmp.path().join(relative)).unwrap(), png);
    assert_eq!(store.get(&idea.id).unwrap(), idea);
    assert_eq!(idea.source.as_deref(), Some("截图来源"));
    assert!(store.capture_image(b"not-png", "坏图", None).is_err());
}

#[test]
fn migration_preserves_legacy_originals_ids_and_is_repeatable() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("old");
    fs::create_dir(&old).unwrap();
    let raw = r#"{"id":"idea-old","title":"旧标题","body":"旧正文\n\n末段\n","tags":["旧标签"],"created_at":"2025-01-02T00:00:00Z","updated_at":"2025-01-03T00:00:00Z"}"#;
    fs::write(old.join("idea-old.json"), raw).unwrap();
    let store = InspirationStore::new(tmp.path().join("new"));
    assert_eq!(store.migrate_from(&old).unwrap(), 1);
    assert_eq!(store.migrate_from(&old).unwrap(), 0);
    assert_eq!(fs::read_to_string(old.join("idea-old.json")).unwrap(), raw);
    let idea = store.get("idea-old").unwrap();
    assert!(idea.path.ends_with("2025-01-02 旧标题.md"));
    assert_eq!(idea.body, "旧正文\n\n末段\n");
    assert_eq!(idea.tags, vec!["旧标签"]);
}

#[cfg(unix)]
#[test]
fn symlinks_and_traversal_cannot_escape_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("notes");
    fs::create_dir(&library).unwrap();
    let outside = tmp.path().join("outside.md");
    fs::write(&outside, "# 外部\n\n不要改").unwrap();
    std::os::unix::fs::symlink(&outside, library.join("link.md")).unwrap();
    let store = InspirationStore::new(&library);
    assert!(store.list().unwrap().is_empty());
    assert!(store.file_path("../outside").is_err());
    assert!(store.delete("../../outside").is_err());
    std::os::unix::fs::symlink(tmp.path(), library.join("附件")).unwrap();
    assert!(store
        .capture_image(b"\x89PNG\r\n\x1a\n", "图", None)
        .is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "# 外部\n\n不要改");
}

#[test]
fn external_frontmatter_without_heading_and_crlf_notes_survive_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let store = InspirationStore::new(tmp.path());
    let prefix = "---\r\ncustom: keep\r\n---\r\n";
    let path = tmp.path().join("plain.md");
    fs::write(&path, format!("{prefix}原正文\r\n")).unwrap();
    let idea = store.list().unwrap().pop().unwrap();
    assert_eq!(idea.body, "原正文\r\n");
    store
        .update(&idea.id, None, Some("新正文\r\n末段\r\n".into()), None)
        .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains(prefix));
    assert!(raw.ends_with("新正文\r\n末段\r\n"));
    fs::write(&path, raw.replace("\n# plain\n\n", "\n# 外部标题\r\n\r\n")).unwrap();
    let reread = store.get(&idea.id).unwrap();
    assert_eq!(reread.title, "外部标题");
    assert_eq!(reread.body, "新正文\r\n末段\r\n");
}
