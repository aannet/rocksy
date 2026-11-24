//! Access policy wired the way the shell will wire it: registry and session
//! read from the data directory once, then every path sent by the "page" is
//! checked before the real `read_file` / `save_file`.

mod common;

use common::TempDir;
use rocksy_core::{
    add_vault, list_vaults, load_opened, load_session, read_file, remove_vault, save_file, save_opened,
    save_session, AccessPolicy, ApiError, ErrorKind, FileContent, PathOrigin, PathStyle, SaveResult, Session,
    VaultRef,
};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

/// The two guarded commands, as `main.rs` will write them.
struct Shell {
    policy: Mutex<AccessPolicy>,
}

impl Shell {
    fn read(&self, path: &str) -> Result<FileContent, ApiError> {
        self.policy.lock().unwrap().check_read(path, PathOrigin::Page)?;
        read_file(Path::new(path))
    }

    fn save(&self, path: &str, content: &str) -> Result<SaveResult, ApiError> {
        self.policy.lock().unwrap().check_write(path, PathOrigin::Page)?;
        save_file(Path::new(path), content, None)
    }
}

fn assert_refused<T: std::fmt::Debug>(result: Result<T, ApiError>) {
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid, "{err}");
}

#[test]
fn access_guards_read_and_save_around_vaults_and_opened_files() {
    let dir = TempDir::new("access");
    let data = dir.path().join("data");
    let vault = dir.path().join("vault");
    dir.write("vault/a.md", "in vault\n");
    dir.write("loose/note.md", "loose\n");
    dir.write("loose/linked.md", "linked\n");
    dir.write("loose/img/p.png", "png");
    let secret = dir.write("private/secret.md", "secret\n");
    add_vault(&data, &vault).unwrap();

    let shell = Shell { policy: Mutex::new(AccessPolicy::new(&list_vaults(&data))) };
    let in_vault = text(&vault.join("a.md"));
    let loose = text(&dir.path().join("loose/note.md"));
    let linked = text(&dir.path().join("loose/linked.md"));

    // Vault: read and write. `..` out of the vault: neither, and nothing is touched.
    assert_eq!(shell.read(&in_vault).unwrap().text, "in vault\n");
    shell.save(&in_vault, "edited\n").unwrap();
    let escape = format!("{}/../private/secret.md", text(&vault));
    assert_refused(shell.read(&escape));
    assert_refused(shell.save(&escape, "overwritten"));
    assert_refused(shell.read(&text(&secret)));
    assert_refused(shell.save(&text(&dir.path().join("private/new.cmd")), "x"));
    assert_eq!(fs::read_to_string(&secret).unwrap(), "secret\n");
    assert!(!dir.path().join("private/new.cmd").exists());

    // A file outside any vault is refused until the user opens it.
    assert_refused(shell.read(&loose));
    shell.policy.lock().unwrap().allow_opened(&loose).unwrap();
    assert_eq!(shell.read(&loose).unwrap().text, "loose\n");
    shell.save(&loose, "loose edited\n").unwrap();
    // Its folder is readable (links, images), not writable.
    assert_eq!(shell.read(&linked).unwrap().text, "linked\n");
    assert_eq!(shell.read(&text(&dir.path().join("loose/img/p.png"))).unwrap().text, "png");
    // ...for text files and images only.
    dir.write("loose/keys.json", "{}");
    assert_refused(shell.read(&text(&dir.path().join("loose/keys.json"))));
    assert_refused(shell.save(&linked, "x"));
    assert_refused(shell.save(&text(&dir.path().join("loose/created.md")), "x"));
    assert_eq!(fs::read_to_string(dir.path().join("loose/linked.md")).unwrap(), "linked\n");
    assert!(!dir.path().join("loose/created.md").exists());

    // Removing the vault from the registry closes it at the next update.
    let left = remove_vault(&data, &vault).unwrap();
    shell.policy.lock().unwrap().set_vaults(&left);
    assert_refused(shell.read(&in_vault));
    assert_refused(shell.save(&in_vault, "x"));
    assert_eq!(fs::read_to_string(vault.join("a.md")).unwrap(), "edited\n");
}

#[test]
fn access_restores_only_what_the_backend_persisted_as_opened() {
    let dir = TempDir::new("access-session");
    let data = dir.path().join("data");
    let vault = dir.path().join("vault");
    dir.write("vault/a.md", "a\n");
    dir.write("loose/b.md", "b\n");
    dir.write("loose/linked.md", "linked\n");
    dir.write("elsewhere/c.md", "c\n");
    add_vault(&data, &vault).unwrap();
    let a = text(&vault.join("a.md"));
    let b = text(&dir.path().join("loose/b.md"));
    let linked = text(&dir.path().join("loose/linked.md"));
    let c = text(&dir.path().join("elsewhere/c.md"));

    // First run: the user opens b.md (dialog) and follows a link to
    // linked.md; the page then sends its session, with one tab it was never
    // given. The backend persists the list of opened files itself.
    let mut first = AccessPolicy::new(&list_vaults(&data));
    assert!(first.allow_opened(&b).unwrap());
    save_opened(&data, &first.opened_files()).unwrap();
    let sent = Session {
        active_vault: Some(text(&vault)),
        tabs: vec![a.clone(), b.clone(), linked.clone(), c.clone()],
        active_tab: Some(c.clone()),
    };
    let kept = first.sanitize_session(&sent);
    assert_eq!(kept.tabs, [a.clone(), b.clone(), linked.clone()]);
    assert_eq!(kept.active_tab, None);
    assert_eq!(kept.active_vault, Some(text(&vault)));
    save_session(&data, &kept).unwrap();

    // Second run: nothing but the registry, the session and the list.
    let mut second = AccessPolicy::new(&list_vaults(&data));
    assert!(second.check_read(&b, PathOrigin::Page).is_err());
    let restored = load_session(&data).unwrap();
    assert_eq!(second.restore_opened(&load_opened(&data), Some(&restored)), 1);
    for tab in &restored.tabs {
        second.check_read(tab, PathOrigin::Page).unwrap();
        read_file(Path::new(tab)).unwrap();
    }
    second.check_write(&a, PathOrigin::Page).unwrap();
    second.check_write(&b, PathOrigin::Page).unwrap();
    // The file reached through a link is still read-only after the restart.
    assert_refused(second.check_write(&linked, PathOrigin::Page));
    // The tab smuggled by the page did not survive.
    assert!(second.check_read(&c, PathOrigin::Page).is_err());

    // A page that writes the smuggled tab into the session gains nothing at
    // the third run either: the list is the backend's.
    save_session(&data, &sent).unwrap();
    let mut third = AccessPolicy::new(&list_vaults(&data));
    third.restore_opened(&load_opened(&data), load_session(&data).as_ref());
    assert!(third.check_read(&c, PathOrigin::Page).is_err());
    assert_refused(third.check_write(&linked, PathOrigin::Page));
}

#[test]
fn access_windows_rules_hold_on_any_platform() {
    let vaults = [VaultRef { path: r"G:\Mon Drive\MonVault".into(), name: "PKM".into() }];
    let mut policy = AccessPolicy::with_style(PathStyle::Windows, &vaults);
    let page = PathOrigin::Page;

    policy.check_read(r"g:\mon drive\MonVault\01__WORK\note.md", page).unwrap();
    policy.check_write(r"\\?\G:\Mon Drive\MonVault\01__WORK\note.md", page).unwrap();
    policy.check_write("G:/Mon Drive/MonVault/01__WORK/../note.md", page).unwrap();
    assert_refused(policy.check_write(r"G:\Mon Drive\MonVault\..\autre\note.md", page));
    assert_refused(policy.check_write(
        r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.cmd",
        page,
    ));

    // SEC-4: every network spelling is refused from the page...
    for unc in [
        r"\\attaquant\s\a.png",
        "//attaquant.example/s/a.png",
        r"\\?\UNC\attaquant\s\a.png",
        "//?/unc/attaquant/s/a.png",
    ] {
        let err = policy.check_read(unc, page).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert!(err.message.contains("network path refused"), "{err}");
        assert!(rocksy_core::access::is_network_path(unc, PathStyle::Windows));
        // ...and stays out of a session sent by the page.
        let session = Session { active_vault: None, tabs: vec![unc.into()], active_tab: Some(unc.into()) };
        assert_eq!(policy.sanitize_session(&session), Session::default());
    }
    // ...while a note on a share handed over by the OS opens and saves.
    policy.check_read(r"\\nas\docs\note.md", PathOrigin::Os).unwrap();
    policy.allow_opened(r"\\nas\docs\note.md").unwrap();
    policy.check_read("//NAS/docs/note.md", page).unwrap();
    policy.check_write(r"\\?\UNC\nas\docs\note.md", page).unwrap();
    assert_refused(policy.check_read(r"\\nas\docs\other.md", page));
}
