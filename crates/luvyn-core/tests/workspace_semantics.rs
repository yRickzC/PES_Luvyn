use luvyn_core::{LuReader, Project, editor, formatter, language, lexer, parser, resolver};
use std::{collections::BTreeMap, fs};

#[test]
fn separate_roots_inferred_modules_types_self_and_source_survive_artifact() {
    let base = tempfile::tempdir().unwrap();
    let docs = base.path().join("docs");
    let target = base.path().join("game");
    fs::create_dir_all(docs.join("documents/auth")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(
        docs.join("luvyn.toml"),
        "sources = [\"documents\"]\n[project]\ntarget = \"../game\"\n",
    )
    .unwrap();
    let player = "class Player\npurpose: player\nfields:\n    id: i64\n    name: String\n    health: f32\n    alive: bool\nrules:\n    - self.health >= 0.0\n    - self.alive == false quando self.health == 0.0\nfunc damage(amount: f32) -> Result<(), DamageError>\nsource:\n    src/player.rs::damage\nclass DamageError\npurpose: damage failure\n";
    fs::write(docs.join("documents/player.lyn"), player).unwrap();
    fs::write(docs.join("documents/auth/auth.lyn"), "import player as game\nclass Auth\npurpose: auth\nexport:\n    func get(players: HashMap<String, Vec<game.Player>>) -> Option<game.Player>\n").unwrap();
    let mut project = Project::open(&docs).unwrap();
    project.build().unwrap();
    assert_eq!(project.target_project_root, target.canonicalize().unwrap());
    assert!(!docs.join(".luvyn/project.lu").exists());
    assert!(!target.join("player.lyn").exists());
    let graph = project.compiled().unwrap();
    let module = graph
        .symbols
        .iter()
        .find(|s| s.kind == "module" && s.qualified == "player")
        .unwrap();
    let class = graph
        .symbols
        .iter()
        .find(|s| s.qualified == "player.Player")
        .unwrap();
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.from == module.id && e.to == class.id && e.relation == "contains")
    );
    assert!(
        graph
            .symbols
            .iter()
            .any(|s| s.kind == "module" && s.qualified == "auth")
    );
    let location =
        editor::symbol_at(&project.graph, "documents/player.lyn", 9, 15, player).unwrap();
    assert_eq!(
        editor::symbol_context(&project.graph, &location.id),
        "field health: f32\nowner: Player"
    );
    let damage = graph.symbols.iter().find(|s| s.name == "damage").unwrap();
    assert_eq!(damage.sections["source"], ["src/player.rs::damage"]);
    let mut reader = LuReader::open(&target.join(".luvyn/project.lu")).unwrap();
    assert_eq!(
        reader.node(&damage.id).unwrap().unwrap().sections["source"],
        damage.sections["source"]
    );
    assert_eq!(project.build().unwrap().parsed, 0);
    assert!(Project::open(&target).unwrap().compiled().is_ok());
}

#[test]
fn generic_unit_parser_validates_arity_and_repeated_locations() {
    let formatted = formatter::format("func set_name( name : String ) -> Result< () , Error >\n");
    assert_eq!(
        formatted,
        "func set_name(name: String) -> Result<(), Error>\n"
    );
    assert_eq!(formatter::format(&formatted), formatted);
    for ty in [
        "Result<(), Error>",
        "HashMap<String, Vec<Option<User>>>",
        "()",
        "Vec<Result<User, Error>>",
        "module.User",
    ] {
        assert!(parser::TypeExpr::parse(ty).is_ok(), "{ty}");
    }
    for ty in [
        "Result<User>",
        "Result<(),>",
        "Vec<>",
        "String<User>",
        "true",
        "false",
        "null",
        "User?",
        "Option<User",
        "bool!",
        "Option",
    ] {
        assert!(parser::TypeExpr::parse(ty).is_err(), "{ty}");
    }
    let text = "type ID = i64\nclass User\nclass Error\nfunc lookup(id: ID, pairs: HashMap<String, User>, pair: Result<(), Error>) -> Result<Option<User>, Error>\n";
    let parsed = parser::parse("users.lyn", text);
    let graph = resolver::resolve(&[parsed], BTreeMap::new());
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    let returns: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.relation == "returns")
        .collect();
    assert_eq!(returns.len(), 2);
    assert!(returns.iter().all(|e| e.location.column > 90));
    let tokens = lexer::tokenize(text);
    let line_offset = text[..text.find("->").unwrap()].encode_utf16().count() as u32;
    assert!(
        tokens
            .iter()
            .any(|t| t.from == line_offset && t.to == line_offset + 2 && t.kind == "operator")
    );
    assert_eq!(language::lookup("true").unwrap().category, "Literals");
    assert_eq!(language::lookup("->").unwrap().category, "Operators");
    for old in ["ID", "Integer", "Number", "Boolean", "Null", "null"] {
        assert!(!language::BUILTINS.contains(&old));
    }
}

#[test]
fn migration_warns_without_rewriting_and_override_is_explicit() {
    let parsed = parser::parse(
        "core.lyn",
        "module Compiler\npurpose: compiler\nfunc lookup() -> User?\nclass User\n",
    );
    assert!(parsed.diagnostics.iter().any(|d| d.code == "L102"));
    assert!(parsed.diagnostics.iter().any(|d| d.code == "L103"));
    let parsed = parser::parse("core.lyn", "module custom.core override\nclass Compiler\n");
    assert_eq!(parsed.namespace, "custom.core");
    assert!(!parsed.diagnostics.iter().any(|d| d.code == "L102"));
}

#[test]
fn target_configuration_reload_and_sources_change_invalidate_cache() {
    let base = tempfile::tempdir().unwrap();
    let docs = base.path().join("docs");
    fs::create_dir_all(docs.join("sources")).unwrap();
    fs::create_dir(base.path().join("target-a")).unwrap();
    fs::create_dir(base.path().join("target-b")).unwrap();
    fs::write(docs.join("sources/user.lyn"), "class User\npurpose: user\n").unwrap();
    fs::write(
        docs.join("luvyn.toml"),
        "sources = [\".\"]\n[project]\ntarget = \"../target-a\"\n",
    )
    .unwrap();
    let mut project = Project::open(&docs).unwrap();
    project.build().unwrap();
    assert!(
        project
            .graph
            .symbols
            .iter()
            .any(|s| s.qualified == "sources.user.User")
    );
    fs::write(
        docs.join("luvyn.toml"),
        "sources = [\"sources\"]\n[project]\ntarget = \"../target-b\"\n",
    )
    .unwrap();
    assert_eq!(project.build().unwrap().parsed, 1);
    assert!(
        project
            .graph
            .symbols
            .iter()
            .any(|s| s.qualified == "user.User")
    );
    assert!(base.path().join("target-a/.luvyn/project.lu").is_file());
    assert!(base.path().join("target-b/.luvyn/project.lu").is_file());
}
