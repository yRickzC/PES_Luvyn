use luvyn_core::{LuReader, Project, editor, formatter, ide, parser, resolver};
use serde_json::json;
use std::{collections::BTreeMap, fs};

#[test]
fn annotations_generics_main_imports_and_binary_keep_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class Entity\npurpose: identified data\n\n@entity\nclass User\npurpose: identity\n\n@service(\"users\", order: 10)\ninterface Repository<T: Entity>\npurpose: persist values\nfields:\n    values: Vec<T>\nexport:\n    func find<R>(values: HashMap<String, Vec<Option<T>>>) -> Result<R, Entity>\n        describe self.values without execution\n\ntype Collection<T> = Vec<T>\npurpose: ordered values\n\nfunc convert<T, R>(value: T) -> R\npurpose: convert representation\n\nmain:\n    purpose:\n        application documentation\n    depends:\n        Repository\n    export:\n        User\n";
    fs::write(dir.path().join("app.lyn"), source).unwrap();
    let formatted = formatter::format(source);
    assert_eq!(formatter::format(&formatted), formatted);
    fs::write(dir.path().join("app.lyn"), &formatted).unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    project.build().unwrap();
    let repository = project
        .graph
        .symbols
        .iter()
        .find(|s| s.name == "Repository")
        .unwrap();
    assert_eq!(repository.generics[0].bound.as_deref(), Some("Entity"));
    assert_eq!(
        repository.annotations[0].arguments,
        ["\"users\"", "order: 10"]
    );
    assert!(project.graph.edges.iter().any(|e| e.relation == "bound"));
    assert!(project.graph.edges.iter().any(|e| e.relation == "export"));
    assert!(
        !project
            .graph
            .symbols
            .iter()
            .any(|s| s.name == "T" || s.name == "R")
    );
    let mut reader = LuReader::open(&project.output_path().unwrap()).unwrap();
    assert_eq!(reader.node(&repository.id).unwrap().unwrap(), *repository);
    let root = ide::handle(&ide::IdeCore::new(project), json!({"op":"graph"})).unwrap();
    assert!(
        root["result"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["kind"] == "main")
    );
    let duplicate = resolver::resolve(
        &[
            parser::parse("a.lyn", "main:\n    purpose: a"),
            parser::parse("b.lyn", "main:\n    purpose: b"),
        ],
        BTreeMap::new(),
    );
    assert!(
        duplicate
            .diagnostics
            .iter()
            .any(|d| d.code == "S008" && d.message == "project can contain only one main block")
    );
    let invalid = parser::parse(
        "bad.lyn",
        "class Broken<T, T>\npurpose: invalid\nfunc bad<R>(id: R) -> Vec<Option<R>>\n",
    );
    assert!(invalid.diagnostics.iter().any(|d| d.code == "L021"));
    for annotation in ["@name", "@name()", "@name(\"a,b\")", "@name(key: 10)"] {
        let parsed = parser::parse(
            "annotation.lyn",
            &format!("{annotation}\nclass Item\npurpose: item\n"),
        );
        assert!(!parsed.diagnostics.iter().any(|d| d.severity == "error"));
        assert_eq!(parsed.symbols[0].annotations[0].name, "name");
    }
}

#[test]
fn unique_names_are_implicit_ambiguous_names_need_imports_and_local_names_win() {
    let graph = resolver::resolve(
        &[
            parser::parse("User.lyn", "class User\npurpose: identity"),
            parser::parse(
                "Service.lyn",
                "class Service\npurpose: consumer\ndepends User\nexport:\n    func find() -> Option<User>\n",
            ),
        ],
        BTreeMap::new(),
    );
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    let user = graph
        .symbols
        .iter()
        .find(|s| s.kind == "class" && s.name == "User")
        .unwrap();
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.relation == "returns" && e.to == user.id)
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.relation == "depends" && e.to == user.id)
    );
    let a = parser::parse("a.lyn", "class User\npurpose: identity\n");
    let b = parser::parse("b.lyn", "class Service\npurpose: consumer\ndepends User\n");
    assert!(!resolver::resolve(&[a.clone(), b.clone()], BTreeMap::new()).has_errors());
    let c = parser::parse("c.lyn", "class User\npurpose: different identity\n");
    assert!(
        resolver::resolve(&[a.clone(), b, c.clone()], BTreeMap::new())
            .diagnostics
            .iter()
            .any(|d| d.code == "S003")
    );
    let b = parser::parse(
        "b.lyn",
        "import a.User as Account\nclass Service\npurpose: consumer\ndepends Account\n",
    );
    assert!(!resolver::resolve(&[a, b, c], BTreeMap::new()).has_errors());
    let local = parser::parse(
        "a.lyn",
        "class User\npurpose: local\nclass Service\npurpose: consumer\ndepends User\n",
    );
    assert!(
        !resolver::resolve(
            &[local, parser::parse("b.lyn", "class User\npurpose: remote")],
            BTreeMap::new()
        )
        .has_errors()
    );
}

#[test]
fn creation_is_real_save_is_conflict_checked_and_ignore_excludes_every_surface() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("luvyn.toml"), "sources = [\"docs\"]\n").unwrap();
    fs::write(
        dir.path().join(".ignore.luvyn"),
        "generated/\ndocs/legacy/\n*.generated.lyn\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("docs/legacy")).unwrap();
    fs::write(
        dir.path().join("docs/legacy/Old.lyn"),
        "class Hidden\npurpose: hidden",
    )
    .unwrap();
    fs::write(
        dir.path().join("docs/hidden.generated.lyn"),
        "class HiddenGenerated\npurpose: hidden",
    )
    .unwrap();
    let state = ide::IdeCore::new(Project::open(dir.path()).unwrap());
    assert!(
        ide::handle(
            &state,
            json!({"op":"edit","file":"docs/Ghost.lyn","text":"class Ghost"})
        )
        .is_err()
    );
    assert!(ide::handle(&state, json!({"op":"create","file":"WrongRoot.lyn"})).is_err());
    assert!(
        ide::handle(
            &state,
            json!({"op":"create","file":"docs/blocked.generated.lyn"})
        )
        .is_err()
    );
    let created = ide::handle(
        &state,
        json!({"op":"create","file":"docs/Real.lyn","text":"class Real\npurpose: original\n"}),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("docs/Real.lyn")).unwrap(),
        created["text"].as_str().unwrap()
    );
    let revision = state.revision.load(std::sync::atomic::Ordering::Relaxed);
    let saved = ide::handle(&state,json!({"op":"save","file":"docs/Real.lyn","hash":created["hash"],"text":"class Real\npurpose: updated\n"})).unwrap();
    assert!(state.revision.load(std::sync::atomic::Ordering::Relaxed) > revision);
    assert!(saved["hash"] != created["hash"]);
    assert!(
        ide::handle(
            &state,
            json!({"op":"save","file":"docs/Real.lyn","hash":created["hash"],"text":"lost data"})
        )
        .is_err()
    );
    let snapshot = ide::handle(&state, json!({"op":"snapshot"})).unwrap();
    assert_eq!(snapshot["files"].as_array().unwrap().len(), 1);
    assert!(
        !snapshot["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.as_str().unwrap().contains("legacy"))
    );
    let search = ide::handle(&state, json!({"op":"search","query":"Hidden"})).unwrap();
    assert!(search["matches"].as_array().unwrap().is_empty());
    ide::handle(&state, json!({"op":"build"})).unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    let graph = project.compiled().unwrap();
    assert!(!graph.symbols.iter().any(|s| s.name.contains("Hidden")));
    let overlays = BTreeMap::from([(
        "docs/hidden.generated.lyn".into(),
        "class Leaked\npurpose: ignored buffer".into(),
    )]);
    project.analyze(&overlays).unwrap();
    assert!(!project.graph.symbols.iter().any(|s| s.name == "Leaked"));
    assert!(
        !editor::completions(&project, "docs/Real.lyn", "depends:\n    ", 2)
            .iter()
            .any(|c| c.label.contains("Hidden"))
    );
}

#[test]
fn old_keywords_have_migration_errors_and_are_absent_from_dictionary() {
    for keyword in [
        "entity",
        "component",
        "service",
        "system",
        "event",
        "concept",
        "struct",
        "responsibilities",
        "exposes",
        "emits",
        "link",
        "functions",
        "related",
        "uses",
    ] {
        assert!(luvyn_core::language::lookup(keyword).is_none(), "{keyword}");
        let parsed = parser::parse(
            "old.lyn",
            &format!("class Owner\npurpose: owner\n{keyword}: Other\n"),
        );
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.code == "L120" && d.suggestion.is_some()),
            "{keyword}"
        );
    }
    let types = parser::parse(
        "types.lyn",
        "class ECS<T>\npurpose: state\nclass World<T>\npurpose: world\nfields:\n    components: ECS<T>\n    values: Vec<Vec<T>>\nfunc find<T>(id: i64) -> Option<T>\npurpose: lookup\n",
    );
    assert!(!resolver::resolve(&[types], BTreeMap::new()).has_errors());
}

#[test]
fn dictionary_examples_compile_and_generic_completions_stay_scoped() {
    for entry in luvyn_core::language::dictionary() {
        // Import examples explicitly document the external files they depend on.
        if matches!(entry.keyword.as_str(), "import" | "as") {
            continue;
        }
        for example in &entry.examples {
            let graph =
                resolver::resolve(&[parser::parse("example.lyn", example)], BTreeMap::new());
            assert!(
                !graph.has_errors(),
                "{}: {:?}",
                entry.keyword,
                graph.diagnostics
            );
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let source = "class Owner<T>\npurpose: generic owner\nfields:\n    values: Vec<T>\nexport:\n    @public\n    func convert<R>(value: T) -> R\n        describe conversion\nclass Other\npurpose: other\n";
    fs::write(dir.path().join("scope.lyn"), source).unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    project.build().unwrap();
    let items = editor::completions(&project, "scope.lyn", source, 7);
    assert!(
        items
            .iter()
            .any(|i| i.label == "T" && i.detail.contains("Generic parameter"))
    );
    assert!(
        items
            .iter()
            .any(|i| i.label == "R" && i.detail.contains("Generic parameter"))
    );
    let other = editor::completions(&project, "scope.lyn", source, 10);
    assert!(!other.iter().any(|i| i.detail.contains("Generic parameter")));
    let context = luvyn_core::query::query(&project.graph, "Owner", &Default::default()).unwrap();
    assert!(
        luvyn_core::query::render_document(&context)
            .unwrap()
            .contains("@public")
    );
    let invalid = resolver::resolve(
        &[parser::parse(
            "bad.lyn",
            "class Owner<T>\npurpose: generic\nclass Other\npurpose: other\nfields:\n    value: T\n",
        )],
        BTreeMap::new(),
    );
    assert!(invalid.diagnostics.iter().any(|d| d.code == "S005"));
}
