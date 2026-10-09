use luvyn_core::{Project, editor};
use std::{collections::BTreeMap, fs};

fn project_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let mut project = Project::open(dir.path()).unwrap();
    project.analyze(&BTreeMap::new()).unwrap();
    (dir, project)
}

fn completion(project: &Project, file: &str, source: &str, symbol: &str) -> editor::Completion {
    let line = source.lines().count() as u32;
    let items = editor::completions_at(project, file, source, line, u32::MAX);
    let available = items
        .iter()
        .map(|item| item.detail.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    items
        .into_iter()
        .find(|item| item.detail.ends_with(symbol))
        .unwrap_or_else(|| panic!("symbol completion {symbol} not found; available: {available}"))
}

#[test]
fn completion_imports_unique_external_symbols_but_keeps_local_and_ambiguous_names_safe() {
    let external_source = "class Consumer\npurpose: use a repository\ndepends:\n    UserRepo";
    let (_dir, project) = project_with(&[
        ("client/Consumer.lyn", external_source),
        (
            "users/UserRepository.lyn",
            "module users override\nclass UserRepository\npurpose: access users\n",
        ),
    ]);
    let item = completion(
        &project,
        "client/Consumer.lyn",
        external_source,
        "users.UserRepository",
    );
    assert_eq!(item.insert, "UserRepository");
    assert_eq!(
        item.import.as_ref().unwrap().text,
        "import users.UserRepository\n"
    );

    let imported_source = "import users.UserRepository\n\nclass Consumer\npurpose: use a repository\ndepends:\n    UserRepo";
    let (_dir, project) = project_with(&[
        ("client/Consumer.lyn", imported_source),
        (
            "users/UserRepository.lyn",
            "module users override\nclass UserRepository\npurpose: access users\n",
        ),
    ]);
    let item = completion(
        &project,
        "client/Consumer.lyn",
        imported_source,
        "users.UserRepository",
    );
    assert_eq!(item.insert, "UserRepository");
    assert!(item.import.is_none());

    let local_source = "class UserRepository\npurpose: local repository\n\nclass Consumer\npurpose: use a repository\ndepends:\n    UserRepo";
    let (_dir, project) = project_with(&[("client.lyn", local_source)]);
    let item = completion(
        &project,
        "client.lyn",
        local_source,
        "client.UserRepository",
    );
    assert_eq!(item.insert, "UserRepository");
    assert!(item.import.is_none());

    let (_dir, project) = project_with(&[
        ("client/Consumer.lyn", external_source),
        (
            "users/UserRepository.lyn",
            "module users override\nclass UserRepository\npurpose: access users\n",
        ),
        (
            "contracts/UserRepository.lyn",
            "module contracts override\nclass UserRepository\npurpose: specify user access\n",
        ),
    ]);
    for module in ["users", "contracts"] {
        let qualified = format!("{module}.UserRepository");
        let item = completion(&project, "client/Consumer.lyn", external_source, &qualified);
        assert_eq!(item.insert, qualified);
        assert!(item.import.is_none());
    }

    let imported_ambiguous = "import users.UserRepository\nimport contracts.UserRepository\n\nclass Consumer\npurpose: use a repository\ndepends:\n    UserRepo";
    let (_dir, project) = project_with(&[
        ("client/Consumer.lyn", imported_ambiguous),
        (
            "users/UserRepository.lyn",
            "module users override\nclass UserRepository\npurpose: access users\n",
        ),
        (
            "contracts/UserRepository.lyn",
            "module contracts override\nclass UserRepository\npurpose: specify user access\n",
        ),
    ]);
    let item = completion(
        &project,
        "client/Consumer.lyn",
        imported_ambiguous,
        "users.UserRepository",
    );
    assert_eq!(item.insert, "users.UserRepository");
    assert!(item.import.is_none());
}

#[test]
fn import_edit_inserts_in_sorted_position_and_refuses_duplicates() {
    let source = "namespace client\nimport users.UserService\n\nclass Consumer\npurpose: use a repository\ndepends:\n    UserRepository";
    let (_dir, project) = project_with(&[
        ("client/Consumer.lyn", source),
        (
            "users/UserRepository.lyn",
            "module users override\nclass UserRepository\npurpose: access users\n",
        ),
        (
            "users/UserService.lyn",
            "module users override\nclass UserService\npurpose: manage users\n",
        ),
    ]);
    let symbol = project
        .graph
        .symbols
        .iter()
        .find(|s| s.qualified == "users.UserRepository")
        .unwrap();
    let edit = editor::import_edit("client/Consumer.lyn", source, symbol);
    assert_eq!(edit.range.start_line_number, 2);
    assert_eq!(edit.text, "import users.UserRepository\n");
    let mut lines: Vec<_> = source.lines().collect();
    lines.insert(
        edit.range.start_line_number as usize - 1,
        edit.text.trim_end(),
    );
    let with_import = lines.join("\n");
    assert!(
        editor::import_edit("client/Consumer.lyn", &with_import, symbol)
            .text
            .is_empty()
    );
}
