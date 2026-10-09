use luvyn_core::{
    LuReader, Project, editor,
    query::{self, QueryOptions},
};
use std::collections::BTreeMap;

#[test]
fn resource_values_resolve_validate_query_build_export_and_autocomplete() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Weapon.lyn"), "enum Rarity\npurpose: nível da arma\nvalues:\n    Common\n    Rare\n\nclass Weapon<T>\npurpose: representar uma arma\nfields:\n    name: String\n    damage: T\n    weight: f32\n    rarity: Rarity\n\nclass Player\npurpose: carregar uma arma\nfields:\n    weapon: Weapon<i32>\n").unwrap();
    let resource = "class Weapon<i32> \"iron_sword\"\npurpose: arma inicial do jogador\nfields:\n    name: \"Iron Sword\"\n    damage: 12\n    weight: 2.5\n    ";
    std::fs::write(dir.path().join("iron_sword.resource.lyn"), resource).unwrap();
    std::fs::write(
        dir.path().join("player.resource.lyn"),
        "class Player \"hero\"\npurpose: jogador inicial\nfields:\n    weapon: iron_sword\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("ignored")).unwrap();
    std::fs::write(
        dir.path().join("ignored/Secret.resource.lyn"),
        "class Weapon \"secret\"\nfields:\n    damage: 1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".ignore.luvyn"), "ignored/\n").unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    project.analyze(&BTreeMap::new()).unwrap();
    assert!(
        !project.graph.has_errors(),
        "{:?}",
        project.graph.diagnostics
    );
    let resource_node = project
        .graph
        .symbols
        .iter()
        .find(|s| s.kind == "resource" && s.name == "iron_sword")
        .unwrap();
    assert!(
        !project
            .discover()
            .unwrap()
            .contains(&"ignored/Secret.resource.lyn".into())
    );
    assert!(
        project
            .graph
            .edges
            .iter()
            .any(|e| e.from == resource_node.id && e.relation == "instance_of")
    );
    assert!(
        project
            .graph
            .edges
            .iter()
            .any(|e| e.relation == "references" && e.to == resource_node.id)
    );
    let result = query::query(&project.graph, "iron_sword", &QueryOptions::default()).unwrap();
    let rendered = query::render(&result, "compact", 2000).unwrap();
    assert!(rendered.contains("iron_sword: Weapon<i32>"), "{rendered}");
    assert!(
        rendered.contains("purpose: arma inicial do jogador"),
        "{rendered}"
    );
    assert!(rendered.contains("damage=12"), "{rendered}");
    let fields = editor::completions_at(&project, "iron_sword.resource.lyn", resource, 7, 5);
    assert!(fields.iter().any(|c| c.label == "rarity"));
    let value = format!("{}    damage: ", resource.strip_suffix("    ").unwrap());
    let values = editor::completions_at(&project, "iron_sword.resource.lyn", &value, 7, 13);
    assert!(values.iter().any(|c| c.label == "42"));
    let enum_value = format!("{}    rarity: ", resource.strip_suffix("    ").unwrap());
    let enum_values =
        editor::completions_at(&project, "iron_sword.resource.lyn", &enum_value, 7, 13);
    assert!(enum_values.iter().any(|c| c.label == "Common"));
    project.build().unwrap();
    let reader = LuReader::open(&dir.path().join(".luvyn/project.lu")).unwrap();
    assert!(
        reader
            .nodes()
            .iter()
            .any(|n| n.kind == "resource" && n.name == "iron_sword")
    );
    let artifact = reader.load().unwrap();
    assert!(artifact.symbols.iter().any(|s| s.kind == "resource" && s.sections["fields"].iter().any(|v| v == "damage: 12")));
    let compiled_query = query::query(&artifact, "iron_sword", &QueryOptions::default()).unwrap();
    assert!(
        query::render(&compiled_query, "compact", 2000)
            .unwrap()
            .contains("damage=12")
    );
    let archive = luvyn_core::export::bytes(&project.graph).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let mut document = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("modules/iron_sword.md").unwrap(),
        &mut document,
    )
    .unwrap();
    assert!(document.contains("damage=12"), "{document}");
}

#[test]
fn resource_diagnostics_reject_unknown_fields_bad_values_and_behavior_blocks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Weapon.lyn"), "class Weapon\npurpose: arma\nfields:\n    damage: i32\n\nclass Box<T>\npurpose: caixa\nfields:\n    value: T\n").unwrap();
    std::fs::write(dir.path().join("bad.resource.lyn"), "class Weapon \"bad\"\npurpose: inválido\nfields:\n    damage: espada\n    missing: 1\nbehavior:\n    executar\n").unwrap();
    let mut project = Project::open(dir.path()).unwrap();
    project.analyze(&BTreeMap::new()).unwrap();
    let codes: Vec<_> = project
        .graph
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.contains(&"S012"), "{:?}", project.graph.diagnostics);
    assert!(codes.contains(&"S013"), "{:?}", project.graph.diagnostics);
    assert!(codes.contains(&"L133"), "{:?}", project.graph.diagnostics);
    assert!(project.build().is_err());
}
