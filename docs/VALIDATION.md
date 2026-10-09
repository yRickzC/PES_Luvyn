# Validação da tela de Projetos— 9 de outubro de 2026

- Escopo restrito: não foram executadas suítes globais nem testes de componentes não alterados.
- Oito testes focados passaram: três de registry/sync, dois do contrato HTTP Drive, dois de abertura CLI/troca de workspace e um do registry Mobile com URI SAF. Atualizações condicionais e conflitos foram verificados sem conta Google.
- TypeScript/Vite desktop/mobile passaram. Clippy dos crates/targets alterados passou sem avisos. Android Java compilou com Google AuthorizationClient; Rust JNI compilou para arm64 e x86_64.
- Browser: abertura sem caminho exibiu Projetos; Novo Local criou pasta, main.lyn e configuração reais; Abrir Local reutilizou arquivos; pesquisa filtrou recentes; remover recente preservou main.lyn; retorno à IDE usou a mesma sessão. O template main.lyn revelou uma colisão de namespace que foi corrigida e coberta pelo teste da criação.
- Executável release desktop instalado em `C:/Users/User/.cargo/bin/luvyn.exe`. Hosts Desktop/Server sem caminho abriram Projetos e liberaram o terminal; `ide . --server` abriu o workspace direto. O recente persistiu após reiniciar o host; Build da UI escreveu artifact real com Problems 0. APK debug atualizado em `dist/android/luvyn-debug.apk` (arm64 + x86_64). Não foram reexecutados testes instrumentados de módulos anteriores.
- Layout Mobile verificado em 390 × 844: início em Projetos, ações Local/Cloud e recentes. Testes JNI preservam URI de provider e persistência privada. Não foi realizado login real Google: clients OAuth não estão configurados neste ambiente. Configure conforme [PROJECTS.md](PROJECTS.md).

# Validação da revisão — 9 de outubro de 2026

Implementação existente preservada, incluindo hosts desktop/server e adapter Android. Linguagem simplificada no Core; artifacts agora usam major 2.

- 31 testes passaram: 12 invariantes gerais do Core, 2 de self, 5 de linguagem/IDE/ignore, 4 de workspace, 4 de servidor/sessão, 3 de CLI e 1 do adapter mobile. `cargo test --workspace --release --locked` passou; após o ajuste de resolução módulo/símbolo, `cargo test -p luvyn-core -p luvyn-mobile --locked` e `cargo test -p luvyn --release --features desktop --locked` também passaram.
- `cargo clippy --workspace --all-targets --features luvyn/desktop --locked -- -D warnings` e `cargo fmt --all -- --check`: passaram. Frontends desktop/mobile passaram em TypeScript + Vite.
- Testes verificam annotations livres, parâmetros locais/bounds, `Vec<Vec<Option<T>>>`, export de métodos, main único, formatter idempotente, metadata em leitura parcial `.lu`, resolução sem imports e colisão User.lyn/class User.
- Criação da IDE verifica arquivo real antes de abrir buffer, source roots, hash de salvamento, recusa de ghost buffers e conflitos verdadeiros. `.ignore.luvyn` foi verificado no Explorer, busca, completion, overlays e artifact.
- Exemplos do dictionary compilados isoladamente; exemplos de import declaram explicitamente os documentos externos necessários.
- Dogfooding: `check/build` do projeto passou com 3 documentos, 25 nodes e 49 edges. `examples/users` passou com 6 documentos, 21 nodes e 26 edges; ZIP semântico gerado com fontes migradas.
- Find foi validado por clique no X, seguido de Build: permaneceu fechado. O tooltip simples do Monaco deixou de interceptar o botão; rich hovers continuam interativos. Busca global retornou duas referências self e Limpar busca manteve zero resultados após Build.
- Validação visual realizada com workspace isolado `.luvyn/validation-small`: criação de World.lyn, confirmação imediata no filesystem, edição de generics/annotations, Ctrl+S, completion self. com somente name/values, Find textual, busca global, Check, Build real e grafo inicial pela raiz main. Metadata @system(order: 10) aparece no inspector.
- Release desktop foi compilado e instalado em `C:\Users\User\.cargo\bin\luvyn.exe`. ABI/schema v1 não é aceito silenciosamente; recompile artifacts com `luvyn build`.
- Android: adapter Rust e fixtures atualizados para a DSL; APK/testes instrumentados não foram reexecutados nesta revisão. A validação anterior abaixo registra o estado de 8 de outubro.

# Histórico: validação de 8 de outubro

Executado em 8 de outubro de 2026 no Windows. Alterações existentes no workspace foram preservadas.

## Core, CLI e hosts

- `cargo test --workspace --features desktop --locked`: 26 testes passaram, incluindo parser, artefato binário, resolver, query, editor, HTTP/sessão, lifecycle e adapter mobile.
- `cargo clippy --workspace --all-targets --features desktop --locked -- -D warnings`: passou sem avisos. `cargo fmt --all --check`: passou.
- Testes de workspace cobrem módulos inferidos, imports/aliases, `Result<(), Error>`, generics, ranges repetidos, `self`, source mappings no `.lu`, migração, cache e troca de Target.
- Teste CLI cria documentação separada com `init --target`, recusa sobrescrever configuração, compila no Target, consulta sem fontes `.lyn` e alterna política Git.
- `luvyn check`, `build`, `get Compiler` e `fmt --check` passaram: três documentos, 20 nodes/39 edges. Exemplo independente `examples/users`: quatro documentos, 17 nodes/25 edges.
- `npm run build`: TypeScript e bundles Desktop/Mobile passaram. UI mobile verificada em viewport 390 × 844: Files, editor real, tipos/arrow, símbolos, dictionary e subgrafo com módulo `player`. `self.` sugeriu fields tipados; toque em `health` completou a referência e Save persistiu pelo Core. O grafo abre com zoom legível e controles de 44 px.
- O adapter mobile também foi validado com `output` personalizado; SAF usa o caminho relativo retornado pelo Core.

## Android

- Projeto real em `apps/ide-mobile`, JNI em `crates/luvyn-mobile`, Core e sessão compartilhados com Desktop/Server.
- Toolchain usada: SDK 36, build-tools 36.0.0, NDK 28.2.13676358, Gradle Wrapper 9.6.1, AGP 9.2.1 e JDK 25. Projeto aceita JDK 17+.
- Rust Core compilou para `aarch64-linux-android` e `x86_64-linux-android`.
- `luvyn ide build --android` passou e gerou `dist/android/luvyn-debug.apk`.
- Dois testes instrumentados passaram no emulator Android 16: JNI consultou dictionary, compilou documentação em Target separado e leu graph; Activity/WebView iniciou. A repetição com o APK final também confirmou `output_relative` em runtime. Execução direta: `adb shell am instrument -w dev.luvyn.mobile.test/androidx.test.runner.AndroidJUnitRunner`, resultado `OK (2 tests)`.
- Primeira execução Gradle de testes falhou ao anexar runner no emulator recém-iniciado. Instalação explícita dos dois APKs e execução direta passaram.

## Limites da primeira versão

APK debug, sem release assinado. SAF usa staging privado e publica somente artefato no Target; rename/escrita em providers não são transações atômicas. Fluxo SAF implementado e compilado, ainda sem validação em todos os providers ou dispositivo físico. Staging antigo permanece para recuperação. Rename de arquivo não reescreve imports automaticamente.

Índice Graphify local atualizado. Extractor não analisou completamente DSL Gradle; build Gradle validou projeto. `LuReader` permite consumir `.lu` sem fontes; não foi criado adapter para produto Graphify externo.
