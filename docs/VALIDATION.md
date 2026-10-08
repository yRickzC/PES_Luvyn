# Validação da entrega

Executado em 8 de outubro de 2026, Windows, Rust/Cargo 1.95 e Node.js 24.

- `cargo fmt --all -- --check`: passou.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passou, sem avisos.
- `cargo test --workspace --locked`: 15 testes passaram. Cobrem parser, resolução/imports, estabilidade de IDs, corrupção/roundtrip/leitura parcial do binário, queries/ciclos/orçamento, ZIP determinístico, cache incremental, formatter, UTF-16, rename seguro, CLI/exit codes, autenticação local e conflitos de save.
- `npm run build`: TypeScript e bundle de produção passaram. Assets e worker distribuídos no executável.
- `cargo install --path crates/luvyn --force --locked`: executável `luvyn` instalado em `C:\Users\User\.cargo\bin`.
- Dogfooding: `check`, `build`, queries exatas/naturais, `export`, `git`, `skill --local` e `fmt --check` passaram. Seis documentos geraram 20 símbolos e 32 relações. Build repetido: zero arquivos parseados, seis reutilizados.
- IDE testada pelo navegador: criou e salvou `.lyn` real, autocomplete encontrou `UserRepository`, inseriu import de outro namespace, diagnostics ficaram zerados, F12 abriu a definição na linha 3, Shift+F12 mostrou referências em dois arquivos. Documento temporário removido após o teste. Grafo interativo mostrou relações e navegação do exemplo.

Artefatos gerados: `.luvyn/project.lu` e `.luvyn/context.zip`. A configuração Git ignora esses artefatos e mantém fontes `.lyn` versionáveis. Skill local: `.agents/skills/luvyn/SKILL.md`.

Limites deliberados: IDE no navegador local; parsing incremental com resolução global linear após mudanças estruturais; orçamento de contexto estimado por caracteres; consultas naturais determinísticas limitadas; compactação preserva prosa sem tentar reescrevê-la. CI Linux/Windows incluída, mas somente o ambiente Windows foi executado nesta entrega.
