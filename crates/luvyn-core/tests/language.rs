use luvyn_core::{LuReader, Project, editor, language, parser, resolver};
use std::collections::BTreeMap;

#[test]
fn self_is_scoped_typed_indexed_and_rename_preserves_prose() {
    let dir = tempfile::tempdir().unwrap();
    let source = "namespace app\nclass User\npurpose: usuário\nfields:\n    id: i64\n    name: String\n    email: String\nrules:\n    - 😀 self.email único; self.email válido\n    - email em prosa fica intacto\n    - self.id imutável\nexport:\n    func setName(name: String) -> User\n        self.name = name\nclass Other\npurpose: outra entidade\nfields:\n    other: bool\nrules:\n    - self.other é verdadeiro\n";
    std::fs::write(dir.path().join("User.lyn"), source).unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    project.build().unwrap();
    let formatted = luvyn_core::formatter::format(source);
    assert!(formatted.contains("        self.name = name"));
    assert_eq!(luvyn_core::formatter::format(&formatted), formatted);
    assert!(
        !resolver::resolve(&[parser::parse("User.lyn", &formatted)], BTreeMap::new()).has_errors()
    );
    assert!(
        !project.graph.has_errors(),
        "{:?}",
        project.graph.diagnostics
    );
    let email = project
        .graph
        .symbols
        .iter()
        .find(|s| s.qualified == "app.User::email")
        .unwrap();
    assert_eq!(email.kind, "field");
    assert_eq!(editor::references(&project.graph, &email.id).len(), 2);
    let method = project
        .graph
        .symbols
        .iter()
        .find(|s| s.qualified == "app.User::setName")
        .unwrap();
    let name = project
        .graph
        .symbols
        .iter()
        .find(|s| s.qualified == "app.User::name")
        .unwrap();
    assert!(
        project
            .graph
            .edges
            .iter()
            .any(|e| e.from == method.id && e.to == name.id && e.relation == "references")
    );
    let edits = editor::rename(
        &project,
        &email.id,
        "address",
        &BTreeMap::from([("User.lyn".into(), source.into())]),
    )
    .unwrap();
    let changed = editor::apply_edits(
        &BTreeMap::from([("User.lyn".into(), source.into())]),
        &edits,
    )
    .unwrap();
    assert!(changed["User.lyn"].contains("self.address único; self.address válido"));
    assert!(changed["User.lyn"].contains("email em prosa fica intacto"));
    assert!(
        editor::rename(
            &project,
            &email.id,
            "id",
            &BTreeMap::from([("User.lyn".into(), source.into())])
        )
        .is_err()
    );
    let mut reader = LuReader::open(&dir.path().join(".luvyn/project.lu")).unwrap();
    assert!(reader.nodes().iter().any(|n| n.id == email.id));
    assert_eq!(
        reader
            .node(&email.id)
            .unwrap()
            .unwrap()
            .signature
            .as_deref(),
        Some("email: String")
    );
    assert!(reader.edges().iter().any(|e| e.to == email.id));
}
#[test]
fn self_completion_hover_scope_invalid_context_and_dictionary_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class User\npurpose: user\nfields:\n    email: String\n    id: i64\nrules:\n    - self.\nexport:\n    func update() -> ()\n        self.email muda\nclass Other\npurpose: other\nfields:\n    other: bool\n";
    std::fs::write(dir.path().join("User.lyn"), source).unwrap();
    let mut p = Project::open(dir.path()).unwrap();
    p.analyze(&BTreeMap::new()).unwrap();
    let items = editor::completions_at(&p, "User.lyn", source, 7, 12);
    let mut names: Vec<_> = items.iter().map(|c| c.label.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["email", "id"]);
    assert!(items.iter().all(|c| c.import.is_none()));
    assert_eq!(
        editor::symbol_at(&p.graph, "User.lyn", 10, 17, source)
            .unwrap()
            .qualified,
        "User.User::email"
    );
    assert_eq!(
        editor::symbol_at(&p.graph, "User.lyn", 10, 10, source)
            .unwrap()
            .qualified,
        "User.User"
    );
    let parsed = parser::parse(
        "invalid.lyn",
        "func standalone() -> ()\npurpose: function\nrules:\n    - self.foo\nclass E\npurpose: test\nfields:\n    id: i64\nrules:\n    - self.foo\n    - \"self.nope\" # self.nope\nvalues:\n    self.id\n",
    );
    let graph = resolver::resolve(&[parsed], BTreeMap::new());
    assert!(graph.diagnostics.iter().any(|d| d.code == "S006"));
    assert!(graph.diagnostics.iter().any(|d| d.code == "S007"));
    assert!(graph.diagnostics.iter().any(|d| d.code == "L016"));
    assert_eq!(
        graph
            .diagnostics
            .iter()
            .filter(|d| d.code == "S007")
            .count(),
        1
    );
    for keyword in language::KINDS
        .iter()
        .chain(language::TEXT_SECTIONS)
        .chain(language::RELATIONS)
        .chain(language::BUILTINS)
        .chain(
            [
                "namespace",
                "import",
                "as",
                "self",
                "export",
                "main",
                "@",
                "generics",
            ]
            .iter(),
        )
    {
        let entry = language::lookup(keyword).unwrap();
        assert!(!entry.syntax.is_empty());
        assert!(!entry.examples.is_empty());
    }
    assert!(language::lookup("function").is_none());
    assert!(
        language::summary(Some("self"))
            .unwrap()
            .contains("self.<field>")
    );
}
