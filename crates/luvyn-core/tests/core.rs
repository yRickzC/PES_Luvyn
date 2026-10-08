use luvyn_core::{
    binary, editor, export, formatter, parser,
    query::{self, QueryOptions},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
};

fn graph() -> Graph {
    let source = "namespace test\n\nentity User\npurpose: identity\n\ninterface Repository\npurpose: persistence\nexposes:\n    save(user: User) -> User\n\nservice UserService\nimplements Repository\npurpose: gerenciar usuários\nrules:\n    - create requires valid email\n    - write records audit\nbehavior:\n    validar email antes de persistir\ndepends:\n    Repository\nexposes:\n    createUser(name: String, email: String) -> User?\n    getUser(id: ID) -> User?\n";
    resolver::resolve(&[parser::parse("docs/users.lyn", source)], BTreeMap::new())
}
#[test]
fn parser_multisymbol_signatures_comments_and_diagnostics() {
    let g = graph();
    assert!(!g.has_errors(), "{:?}", g.diagnostics);
    assert_eq!(g.symbols.len(), 6);
    assert!(g.edges.iter().any(|e| e.relation == "returns"));
    assert!(g.edges.iter().any(|e| e.relation == "exposes"));
    let f = parser::parse(
        "broken.lyn",
        "# comment\nservice Good\npurpose: https://example.com # ignored\nexposes:\n    broken(arg) -> Ghost\nunknown:\n    value\n",
    );
    assert_eq!(f.symbols[0].sections["purpose"], ["https://example.com"]);
    assert!(
        f.diagnostics
            .iter()
            .any(|d| d.code == "L011" && d.location.file == "broken.lyn" && d.location.line == 5)
    );
    assert!(f.diagnostics.iter().any(|d| d.code == "L007"));
    let f = parser::parse("x.lyn", "service X\npurpose: \"literal # not comment\"\n");
    assert_eq!(
        f.symbols[0].sections["purpose"],
        ["\"literal # not comment\""]
    );
}
#[test]
fn resolver_imports_aliases_ambiguity_duplicates_and_stable_ids() {
    let a = parser::parse("a.lyn", "namespace one\nentity User\npurpose: identity\n");
    let b = parser::parse(
        "b.lyn",
        "namespace two\nimport one.User as Person\nservice S\npurpose: consumer\ndepends Person\nexposes:\n    use(person: Person) -> one.User\n",
    );
    let g = resolver::resolve(&[a.clone(), b], BTreeMap::new());
    assert!(!g.has_errors(), "{:?}", g.diagnostics);
    let missing = parser::parse(
        "c.lyn",
        "namespace two\nservice S\npurpose: consumer\ndepends User\n",
    );
    assert!(
        resolver::resolve(&[a.clone(), missing], BTreeMap::new())
            .diagnostics
            .iter()
            .any(|d| d.code == "S004")
    );
    let duplicate = resolver::resolve(&[a.clone(), a], BTreeMap::new());
    assert!(duplicate.diagnostics.iter().any(|d| d.code == "S001"));
    let ambiguous = resolver::resolve(
        &[
            parser::parse("a.lyn", "namespace one\nentity User\npurpose: x"),
            parser::parse("b.lyn", "namespace two\nentity User\npurpose: x"),
            parser::parse(
                "c.lyn",
                "namespace three\nimport one.User\nimport two.User\nservice S\npurpose: x\ndepends User",
            ),
        ],
        BTreeMap::new(),
    );
    assert!(ambiguous.diagnostics.iter().any(|d| d.code == "S003"));
    assert_eq!(
        stable_id("one.User", "entity"),
        g.symbols
            .iter()
            .find(|s| s.qualified == "one.User")
            .unwrap()
            .id
    );
    let moved = resolver::resolve(
        &[parser::parse(
            "moved.lyn",
            "\nnamespace one\n\nentity User\npurpose: changed wording\n",
        )],
        BTreeMap::new(),
    );
    assert_eq!(moved.symbols[0].id, stable_id("one.User", "entity"));
}
#[test]
fn indexed_queries_fuzzy_natural_paths_cycles_and_budget() {
    let g = graph();
    let options = QueryOptions::default();
    let exact = query::query(&g, "UserService", &options).unwrap();
    assert!(exact.symbols.iter().any(|s| s.name == "createUser"));
    assert!(exact.symbols.len() < g.symbols.len());
    let reverse = query::query(&g, "quem depende de Repository", &options).unwrap();
    assert!(reverse.symbols.iter().any(|s| s.name == "UserService"));
    assert!(!reverse.symbols.iter().any(|s| s.name == "User"));
    let api = query::query(&g, "o que UserService expõe", &options).unwrap();
    assert_eq!(api.symbols.len(), 3);
    assert_eq!(
        query::query(
            &g,
            "UserServic",
            &QueryOptions {
                depth: 0,
                limit: 1,
                ..options.clone()
            }
        )
        .unwrap()
        .symbols[0]
            .name,
        "UserService"
    );
    let path = query::query(&g, "path UserService -> User", &options).unwrap();
    assert_eq!(path.symbols.first().unwrap().name, "UserService");
    assert_eq!(path.symbols.last().unwrap().name, "User");
    assert!(query::query(&g, "missing-nothing", &options).is_err());
    let cyclic = resolver::resolve(
        &[parser::parse(
            "cycle.lyn",
            "service A\npurpose: a\ndepends B\nservice B\npurpose: b\ndepends A",
        )],
        BTreeMap::new(),
    );
    assert_eq!(
        query::query(
            &cyclic,
            "neighbors A",
            &QueryOptions {
                depth: 8,
                ..options.clone()
            }
        )
        .unwrap()
        .symbols
        .len(),
        2
    );
    let text = query::render(&exact, "compact", 64).unwrap();
    assert!(text.contains("budget") || text.chars().count() <= 256);
    let json = query::render(&exact, "json", 10).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
}
#[test]
fn binary_deterministic_roundtrip_partial_reads_and_corruption() {
    let g = graph();
    let bytes = binary::encode(&g).unwrap();
    assert_eq!(bytes, binary::encode(&g).unwrap());
    assert_ne!(bytes.first(), Some(&b'{'));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.lu");
    fs::write(&path, &bytes).unwrap();
    let loaded = binary::read(&path).unwrap();
    assert_eq!(g.symbols, loaded.symbols);
    assert_eq!(g.edges, loaded.edges);
    let mut artifact = binary::Artifact::open(&path).unwrap();
    assert_eq!(
        artifact.node(&g.symbols[0].id).unwrap().unwrap(),
        g.symbols[0]
    );
    let partial = artifact
        .query("UserService", &QueryOptions::default())
        .unwrap();
    let full = query::query(&g, "UserService", &QueryOptions::default()).unwrap();
    assert_eq!(partial.symbols, full.symbols);
    for (index, value) in [
        (0, 0),
        (8, 99),
        (12, 255),
        (24, 10),
        (bytes.len() - 1, bytes[bytes.len() - 1] ^ 255),
    ] {
        let mut broken = bytes.clone();
        broken[index] = value;
        fs::write(&path, broken).unwrap();
        assert!(binary::read(&path).is_err(), "corruption at {index}");
    }
    fs::write(&path, &bytes[..20]).unwrap();
    assert!(binary::read(&path).is_err());
    // Directory-only access must not decode a different node's damaged payload.
    let mut artifact_bytes = bytes.clone();
    *artifact_bytes.last_mut().unwrap() ^= 255;
    fs::write(&path, artifact_bytes).unwrap();
    let mut partial = binary::Artifact::open(&path).unwrap();
    assert!(partial.node(&g.symbols[0].id).is_ok());
}
#[test]
fn export_is_deterministic_semantic_and_lossless() {
    let g = graph();
    let bytes = export::bytes(&g).unwrap();
    assert_eq!(bytes, export::bytes(&g).unwrap());
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("INDEX.md").is_ok());
    let mut document = String::new();
    zip.by_name("modules/test.md")
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    assert!(document.contains("User? createUser name:String email:String"));
    assert!(document.contains("create requires valid email"));
    assert!(document.contains("validar email antes de persistir"));
    assert!(document.contains("# namespace test"));
    assert!(document.contains("implements: Repository"));
    assert!(!document.contains("exposes:\n"));
}
#[test]
fn incremental_build_ignores_io_failures_and_preserves_last_good_artifact() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.lyn"), "service A\npurpose: a\n").unwrap();
    fs::write(
        dir.path().join("b.lyn"),
        "service B\npurpose: b\ndepends A\n",
    )
    .unwrap();
    let mut p = Project::open(dir.path()).unwrap();
    assert_eq!(p.build().unwrap().parsed, 2);
    let bytes = fs::read(dir.path().join(".luvyn/project.lu")).unwrap();
    assert_eq!(p.build().unwrap().reused, 2);
    fs::write(dir.path().join("a.lyn"), "service A\npurpose: different\n").unwrap();
    let stats = p.build().unwrap();
    assert_eq!(stats.parsed, 1);
    assert_eq!(stats.reused, 1);
    let mut reopened = Project::open(dir.path()).unwrap();
    assert_eq!(reopened.build().unwrap().parsed, 0);
    let last_good = fs::read(dir.path().join(".luvyn/project.lu")).unwrap();
    assert_ne!(bytes, last_good);
    fs::write(
        dir.path().join("b.lyn"),
        "service B\npurpose: b\ndepends Missing\n",
    )
    .unwrap();
    assert!(p.build().is_err());
    assert_eq!(
        fs::read(dir.path().join(".luvyn/project.lu")).unwrap(),
        last_good
    );
    fs::write(dir.path().join(".gitignore"), "ignored.lyn\n").unwrap();
    fs::write(dir.path().join("ignored.lyn"), "invalid").unwrap();
    assert!(!p.discover().unwrap().contains(&"ignored.lyn".into()));
    fs::write(dir.path().join("bad.lyn"), [255, 254]).unwrap();
    p.analyze(&BTreeMap::new()).unwrap();
    assert!(p.graph.diagnostics.iter().any(|d| d.code == "I002"));
    assert!(p.safe_path("../escape.lyn").is_err());
    assert!(p.safe_path(".git/config").is_err());
}
#[test]
fn editor_imports_completion_utf16_and_safe_rename() {
    let dir = tempfile::tempdir().unwrap();
    let a = "namespace one\nentity User\npurpose: identity\n";
    let b = "namespace two\nservice S\npurpose: consumer\ndepends User\n";
    fs::write(dir.path().join("a.lyn"), a).unwrap();
    fs::write(dir.path().join("b.lyn"), b).unwrap();
    let mut p = Project::open(dir.path()).unwrap();
    p.analyze(&BTreeMap::new()).unwrap();
    let completions = editor::completions(&p, "b.lyn", b, 5);
    assert!(
        completions
            .iter()
            .any(|c| c.label == "User" && c.import.is_some())
    );
    let imports = editor::missing_imports(&p, "b.lyn", b);
    let sources = BTreeMap::from([("a.lyn".into(), a.into()), ("b.lyn".into(), b.into())]);
    let changed = editor::apply_edits(&sources, &imports).unwrap();
    p.analyze(&changed).unwrap();
    assert!(!p.graph.has_errors());
    let id = p
        .graph
        .symbols
        .iter()
        .find(|s| s.name == "User")
        .unwrap()
        .id
        .clone();
    let edits = editor::rename(&p, &id, "Person", &changed).unwrap();
    let changed = editor::apply_edits(&changed, &edits).unwrap();
    assert!(changed["a.lyn"].contains("entity Person"));
    assert!(changed["b.lyn"].contains("import one.Person"));
    assert!(changed["b.lyn"].contains("depends Person"));
    assert!(editor::rename(&p, &id, "bad name", &sources).is_err());
    let location = Location {
        file: "x".into(),
        line: 1,
        column: 3,
        length: 4,
    };
    let range = editor::range(&location, "😀 User");
    assert_eq!(range.start_column, 4);
    assert_eq!(range.end_column, 8);
}
#[test]
fn rename_updates_repeated_signature_types_and_not_prose() {
    let dir = tempfile::tempdir().unwrap();
    let text = "entity User\npurpose: User identity\nservice S\npurpose: User manager\nexposes:\n    pair(left: User, right: User) -> User\n";
    fs::write(dir.path().join("x.lyn"), text).unwrap();
    let mut p = Project::open(dir.path()).unwrap();
    p.analyze(&BTreeMap::new()).unwrap();
    let id = p
        .graph
        .symbols
        .iter()
        .find(|s| s.name == "User")
        .unwrap()
        .id
        .clone();
    let sources = BTreeMap::from([("x.lyn".into(), text.into())]);
    let edits = editor::rename(&p, &id, "Account", &sources).unwrap();
    let result = editor::apply_edits(&sources, &edits).unwrap();
    assert!(result["x.lyn"].contains("pair(left: Account, right: Account) -> Account"));
    assert!(result["x.lyn"].contains("purpose: User manager"));
}
#[test]
fn formatter_is_idempotent_and_preserves_comments() {
    let input = "service S  \n\n\npurpose:\n  hello # explanation \n# keep\n\n";
    let formatted = formatter::format(input);
    assert_eq!(formatted, formatter::format(&formatted));
    assert!(formatted.contains("    hello # explanation"));
    assert!(formatted.contains("# keep"));
    assert_eq!(
        parser::parse("x", input).symbols,
        parser::parse("x", &formatted)
            .symbols
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.location = parser::parse("x", input).symbols[0].location.clone();
                s.end_line = parser::parse("x", input).symbols[0].end_line;
                s
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn unicode_hover_and_global_export_do_not_collide_with_root_namespace() {
    let global = parser::parse("global.lyn", "entity User\npurpose: identity\n");
    let named = parser::parse(
        "named.lyn",
        "namespace root\nentity User\npurpose: distinct identity\n",
    );
    let graph = resolver::resolve(std::slice::from_ref(&global), BTreeMap::new());
    let symbol = editor::symbol_at(&graph, "prose.lyn", 1, 5, "😀 User").unwrap();
    assert_eq!(symbol.name, "User");
    let graph = resolver::resolve(&[global, named], BTreeMap::new());
    let mut zip = zip::ZipArchive::new(Cursor::new(export::bytes(&graph).unwrap())).unwrap();
    assert!(zip.by_name("GLOBAL.md").is_ok());
    assert!(zip.by_name("modules/root.md").is_ok());
}

#[test]
fn flow_queries_rank_behavior_and_work_through_binary_index() {
    let graph = graph();
    let options = QueryOptions::default();
    let result = query::query(&graph, "fluxo de criação de usuário", &options).unwrap();
    assert_eq!(result.symbols[0].name, "UserService");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.lu");
    binary::write(&path, &graph).unwrap();
    let mut artifact = binary::Artifact::open(&path).unwrap();
    let result = artifact
        .query("fluxo de criacao de usuario", &options)
        .unwrap();
    assert_eq!(result.symbols[0].name, "UserService");
}
